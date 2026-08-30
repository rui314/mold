//! Link-time optimization through the linker plugin interface.
//!
//! With `-flto`, compilers emit files holding their intermediate
//! representation instead of machine code: GCC wraps its IR in an ELF
//! file, LLVM writes bitcode. Such files can't be linked directly. The
//! compiler provides a linker plugin (`-plugin`) instead, and the link
//! goes like this:
//!
//! 1. Symbols are read from ELF objects as usual and from IR objects
//!    through the plugin, and resolved together.
//! 2. Once the set of input files is known, the plugin is told how every
//!    IR symbol was resolved and compiles all IR objects into a few ELF
//!    objects.
//! 3. The IR objects are dropped, the compiled objects take their place,
//!    and linking continues as usual.
//!
//! The plugin API (https://gcc.gnu.org/wiki/whopr/driver) is a set of C
//! callbacks: `onload` receives a table of linker-provided functions and
//! registers the plugin's hooks, and results are "returned" by calling
//! back into the linker. The plugin isn't thread-safe and keeps global
//! state, so this module does too.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

use rayon::prelude::*;

use crate::arch::Arch;
use crate::args::VERSION;
use crate::context::Context;
use crate::diagnostics::Diagnostics;
use crate::elf::*;
use crate::input_files::{FileId, ObjId, ObjectFile};
use crate::mapped_file::{must_open_file, MappedFile};
use crate::symbol::SymbolId;
use crate::util::leak_bytes;
use crate::{fatal, out, warn};

// Status codes.
const LDPS_OK: c_int = 0;
const LDPS_NO_SYMS: c_int = 1;
const LDPS_BAD_HANDLE: c_int = 2;

// Transfer vector tags.
const LDPT_NULL: c_int = 0;
const LDPT_LINKER_OUTPUT: c_int = 3;
const LDPT_OPTION: c_int = 4;
const LDPT_REGISTER_CLAIM_FILE_HOOK: c_int = 5;
const LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK: c_int = 6;
const LDPT_REGISTER_CLEANUP_HOOK: c_int = 7;
const LDPT_ADD_SYMBOLS: c_int = 8;
const LDPT_GET_SYMBOLS: c_int = 9;
const LDPT_ADD_INPUT_FILE: c_int = 10;
const LDPT_MESSAGE: c_int = 11;
const LDPT_GET_INPUT_FILE: c_int = 12;
const LDPT_RELEASE_INPUT_FILE: c_int = 13;
const LDPT_ADD_INPUT_LIBRARY: c_int = 14;
const LDPT_OUTPUT_NAME: c_int = 15;
const LDPT_SET_EXTRA_LIBRARY_PATH: c_int = 16;
const LDPT_GET_VIEW: c_int = 18;
const LDPT_GET_INPUT_SECTION_COUNT: c_int = 19;
const LDPT_GET_INPUT_SECTION_TYPE: c_int = 20;
const LDPT_GET_INPUT_SECTION_NAME: c_int = 21;
const LDPT_GET_INPUT_SECTION_CONTENTS: c_int = 22;
const LDPT_UPDATE_SECTION_ORDER: c_int = 23;
const LDPT_ALLOW_SECTION_ORDERING: c_int = 24;
const LDPT_GET_SYMBOLS_V2: c_int = 25;
const LDPT_ALLOW_UNIQUE_SEGMENT_FOR_SECTIONS: c_int = 26;
const LDPT_UNIQUE_SEGMENT_FOR_SECTIONS: c_int = 27;
const LDPT_GET_SYMBOLS_V3: c_int = 28;
const LDPT_GET_INPUT_SECTION_ALIGNMENT: c_int = 29;
const LDPT_GET_INPUT_SECTION_SIZE: c_int = 30;
const LDPT_REGISTER_NEW_INPUT_HOOK: c_int = 31;
const LDPT_GET_WRAP_SYMBOLS: c_int = 32;
const LDPT_ADD_SYMBOLS_V2: c_int = 33;
const LDPT_GET_API_VERSION: c_int = 34;

// Output file types.
const LDPO_EXEC: c_int = 1;
const LDPO_DYN: c_int = 2;
const LDPO_PIE: c_int = 3;

// Symbol kinds.
const LDPK_DEF: u8 = 0;
const LDPK_WEAKDEF: u8 = 1;
const LDPK_WEAKUNDEF: u8 = 3;
const LDPK_COMMON: u8 = 4;

// Symbol types.
const LDST_FUNCTION: u8 = 1;
const LDST_VARIABLE: u8 = 2;

// Symbol visibilities.
const LDPV_PROTECTED: i32 = 1;
const LDPV_INTERNAL: i32 = 2;
const LDPV_HIDDEN: i32 = 3;

// Symbol resolutions.
const LDPR_UNDEF: i32 = 1;
const LDPR_PREVAILING_DEF: i32 = 2;
const LDPR_PREVAILING_DEF_IRONLY: i32 = 3;
const LDPR_PREEMPTED_REG: i32 = 4;
const LDPR_PREEMPTED_IR: i32 = 5;
const LDPR_RESOLVED_IR: i32 = 6;
const LDPR_RESOLVED_EXEC: i32 = 7;
const LDPR_RESOLVED_DYN: i32 = 8;
const LDPR_PREVAILING_DEF_IRONLY_EXP: i32 = 9;

// Message levels.
const LDPL_INFO: c_int = 0;
const LDPL_WARNING: c_int = 1;

// Linker API versions.
const LAPI_V0: c_int = 0;
const LAPI_V1: c_int = 1;

#[repr(C)]
union TagData {
    val: c_int,
    ptr: *const c_void,
}

/// An entry of the transfer vector handed to `onload`.
#[repr(C)]
struct TagValue {
    tag: c_int,
    data: TagData,
}

impl TagValue {
    fn int(tag: c_int, val: c_int) -> TagValue {
        TagValue {
            tag,
            data: TagData { val },
        }
    }

    fn ptr(tag: c_int, ptr: *const c_void) -> TagValue {
        TagValue {
            tag,
            data: TagData { ptr },
        }
    }
}

#[repr(C)]
struct PluginInputFile {
    name: *const c_char,
    fd: c_int,
    offset: u64,
    filesize: u64,
    handle: *mut c_void,
}

#[repr(C)]
struct PluginSection {
    handle: *const c_void,
    shndx: u32,
}

#[cfg(target_endian = "little")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SymbolKinds {
    def: u8,
    symbol_type: u8,
    section_kind: u8,
    padding: u8,
}

#[cfg(target_endian = "big")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SymbolKinds {
    padding: u8,
    section_kind: u8,
    symbol_type: u8,
    def: u8,
}

#[repr(C)]
struct PluginSymbol {
    name: *mut c_char,
    version: *mut c_char,
    kinds: SymbolKinds,
    visibility: i32,
    size: u64,
    comdat_key: *mut c_char,
    resolution: i32,
}

type OnloadFn = unsafe extern "C" fn(*const TagValue) -> c_int;
type ClaimFileHandler = unsafe extern "C" fn(*const PluginInputFile, *mut c_int) -> c_int;
type Hook = unsafe extern "C" fn() -> c_int;
type NewInputHandler = unsafe extern "C" fn(*const PluginInputFile) -> c_int;

extern "C" {
    /// The printf-like diagnostics callback, defined in `csrc/lto-message.c`.
    fn mold_lto_message(level: c_int, fmt: *const c_char, ...) -> c_int;
}

/// The hooks the plugin registers during `onload`.
#[derive(Default)]
struct Hooks {
    claim_file: Option<ClaimFileHandler>,
    all_symbols_read: Option<Hook>,
    cleanup: Option<Hook>,
    /// Whether the plugin speaks the GCC linker API v1, and hence
    /// supports `get_symbols_v3`.
    gcc_api_v1: bool,
}

/// A symbol the plugin reported for the file being claimed.
struct ClaimedSymbol {
    name: Vec<u8>,
    comdat_key: Option<Vec<u8>>,
    def: u8,
    symbol_type: u8,
    visibility: i32,
    size: u64,
}

impl ClaimedSymbol {
    unsafe fn from_plugin(sym: &PluginSymbol) -> ClaimedSymbol {
        let bytes = |p: *const c_char| CStr::from_ptr(p).to_bytes().to_vec();
        ClaimedSymbol {
            name: bytes(sym.name),
            comdat_key: (!sym.comdat_key.is_null()).then(|| bytes(sym.comdat_key)),
            def: sym.kinds.def,
            symbol_type: sym.kinds.symbol_type,
            visibility: sym.visibility,
            size: sym.size,
        }
    }

    /// An IR symbol as an ELF symbol; definitions are absolute, since an
    /// IR object has no sections.
    fn to_elf_sym(&self) -> ElfSym {
        let mut esym = ElfSym {
            st_size: self.size,
            ..ElfSym::default()
        };
        esym.st_shndx = match self.def {
            LDPK_DEF | LDPK_WEAKDEF => SHN_ABS as u16,
            LDPK_COMMON => SHN_COMMON as u16,
            _ => SHN_UNDEF as u16,
        };
        if matches!(self.def, LDPK_WEAKDEF | LDPK_WEAKUNDEF) {
            esym.set_bind(STB_WEAK);
        }
        match self.symbol_type {
            LDST_FUNCTION => esym.set_type(STT_FUNC),
            LDST_VARIABLE => esym.set_type(STT_OBJECT),
            _ => {}
        }
        match self.visibility {
            LDPV_PROTECTED => esym.set_visibility(STV_PROTECTED),
            LDPV_INTERNAL => esym.set_visibility(STV_INTERNAL),
            LDPV_HIDDEN => esym.set_visibility(STV_HIDDEN),
            _ => {}
        }
        esym
    }
}

static LOADED: OnceLock<()> = OnceLock::new();
static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    claim_file: None,
    all_symbols_read: None,
    cleanup: None,
    gcc_api_v1: false,
});

/// Claims are serialized: the plugin hands back the symbols of the file
/// being claimed through this buffer.
static CLAIM_LOCK: Mutex<()> = Mutex::new(());
static CLAIMED_SYMBOLS: Mutex<Vec<ClaimedSymbol>> = Mutex::new(Vec::new());

/// The linker's state, for callbacks. `DIAG` is set once the plugin is
/// loaded; `CONTEXT` only while the plugin is compiling, when nothing
/// else touches the context.
static DIAG: AtomicPtr<Diagnostics> = AtomicPtr::new(ptr::null_mut());
static CONTEXT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

fn diag() -> &'static Diagnostics {
    // SAFETY: set in `load_plugin` to the diagnostics of the context,
    // which outlives the plugin.
    unsafe { &*DIAG.load(Ordering::Acquire) }
}

/// Reports a message the plugin formatted.
///
/// # Safety
///
/// `msg` must point to a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mold_lto_report(level: c_int, msg: *const c_char) {
    let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    match level {
        LDPL_INFO => out!(diag(), "{msg}"),
        LDPL_WARNING => warn!(diag(), "{msg}"),
        _ => fatal!(diag(), "{msg}"),
    }
}

unsafe extern "C" fn register_claim_file_hook(f: ClaimFileHandler) -> c_int {
    HOOKS.lock().unwrap().claim_file = Some(f);
    LDPS_OK
}

unsafe extern "C" fn register_all_symbols_read_hook(f: Hook) -> c_int {
    HOOKS.lock().unwrap().all_symbols_read = Some(f);
    LDPS_OK
}

unsafe extern "C" fn register_cleanup_hook(f: Hook) -> c_int {
    HOOKS.lock().unwrap().cleanup = Some(f);
    LDPS_OK
}

unsafe extern "C" fn add_symbols(
    _handle: *mut c_void,
    nsyms: c_int,
    psyms: *const PluginSymbol,
) -> c_int {
    let syms = std::slice::from_raw_parts(psyms, nsyms as usize);
    *CLAIMED_SYMBOLS.lock().unwrap() = syms.iter().map(|s| ClaimedSymbol::from_plugin(s)).collect();
    LDPS_OK
}

/// Receives an object file the plugin compiled.
unsafe extern "C" fn add_input_file<E: Arch>(path: *const c_char) -> c_int {
    let ctx = &mut *(CONTEXT.load(Ordering::Acquire) as *mut Context<E>);
    let path = CStr::from_ptr(path).to_string_lossy().into_owned();
    let mf = must_open_file(&ctx.diag, "", &path);
    mf.set_dependency(false);

    let mut file = ObjectFile::new::<E>(&ctx.diag, mf, String::new());
    file.is_lto_output = true;
    file.base.set_reachable(true);
    file.base.priority = ctx.file_by_priority.len() as u32;
    file.register_global_symbols::<E>(&ctx.args, &mut ctx.symbol_bin());
    ctx.file_by_priority
        .push(Some(FileId::Obj(ObjId(ctx.objs.len() as u32))));
    ctx.objs.push(Box::new(file));
    LDPS_OK
}

unsafe extern "C" fn get_input_file(_handle: *const c_void, _file: *mut PluginInputFile) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn release_input_file(_handle: *const c_void) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn add_input_library(_path: *const c_char) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn set_extra_library_path(_path: *const c_char) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_view(handle: *const c_void, view: *mut *const c_void) -> c_int {
    let mf = &*(handle as *const MappedFile);
    *view = mf.data().as_ptr() as *const c_void;
    LDPS_OK
}

unsafe extern "C" fn get_input_section_count(_handle: *const c_void, _count: *mut c_int) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_input_section_type(_section: PluginSection, _ty: *mut c_int) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_input_section_name(
    _section: PluginSection,
    _name: *mut *mut c_char,
) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_input_section_contents(
    _section: PluginSection,
    _contents: *mut *const c_char,
    _len: *mut usize,
) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn update_section_order(_sections: *const PluginSection, _num: c_int) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn allow_section_ordering() -> c_int {
    LDPS_OK
}

unsafe extern "C" fn allow_unique_segment_for_sections() -> c_int {
    LDPS_OK
}

unsafe extern "C" fn unique_segment_for_sections(
    _name: *const c_char,
    _flags: u64,
    _align: u64,
    _sections: *const PluginSection,
    _num: c_int,
) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_input_section_alignment(
    _section: PluginSection,
    _align: *mut c_int,
) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_input_section_size(_section: PluginSection, _size: *mut u64) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn register_new_input_hook(_f: NewInputHandler) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_wrap_symbols(_num: *mut u64, _syms: *mut *const *const c_char) -> c_int {
    LDPS_OK
}

unsafe extern "C" fn get_symbols_v1(
    _handle: *const c_void,
    _nsyms: c_int,
    _psyms: *mut PluginSymbol,
) -> c_int {
    unreachable!("the v1 get_symbols API is never offered to the plugin")
}

unsafe extern "C" fn get_symbols_v2<E: Arch>(
    handle: *const c_void,
    nsyms: c_int,
    psyms: *mut PluginSymbol,
) -> c_int {
    get_symbols::<E>(handle, nsyms, psyms, true)
}

unsafe extern "C" fn get_symbols_v3<E: Arch>(
    handle: *const c_void,
    nsyms: c_int,
    psyms: *mut PluginSymbol,
) -> c_int {
    get_symbols::<E>(handle, nsyms, psyms, false)
}

/// Tells the plugin how the symbols of an IR object were resolved. The
/// backend uses this to decide what it must keep: a definition that no
/// regular object refers to may be inlined away, for example.
unsafe fn get_symbols<E: Arch>(
    handle: *const c_void,
    nsyms: c_int,
    psyms: *mut PluginSymbol,
    is_v2: bool,
) -> c_int {
    let ctx = &*(CONTEXT.load(Ordering::Acquire) as *const Context<E>);
    let psyms = std::slice::from_raw_parts_mut(psyms, nsyms as usize);
    let handle = handle as *const MappedFile;
    let Some((fi, file)) = ctx
        .objs
        .iter()
        .enumerate()
        .find(|(_, f)| f.base.mf.is_some_and(|mf| ptr::eq(mf, handle)))
    else {
        return LDPS_BAD_HANDLE;
    };

    // An archive member that wasn't extracted contributes nothing.
    if !file.base.is_reachable() {
        for psym in psyms {
            psym.resolution = LDPR_PREEMPTED_REG;
        }
        return LDPS_NO_SYMS;
    }

    let this = FileId::Obj(ObjId(fi as u32));
    for (i, psym) in psyms.iter_mut().enumerate() {
        let esym = &file.base.elf_syms.at(i + 1);
        let sym = &ctx.symbols[file.base.symbols[i + 1]];
        psym.resolution = match sym.file() {
            None => LDPR_UNDEF,
            Some(owner) if owner == this => {
                if sym.referenced_by_regular_obj() {
                    LDPR_PREVAILING_DEF
                } else if sym.is_exported() {
                    if is_v2 {
                        LDPR_PREVAILING_DEF
                    } else {
                        LDPR_PREVAILING_DEF_IRONLY_EXP
                    }
                } else {
                    LDPR_PREVAILING_DEF_IRONLY
                }
            }
            Some(FileId::Dso(_)) => LDPR_RESOLVED_DYN,
            Some(FileId::Obj(owner)) => {
                let in_ir = ctx.objs[owner.index()].is_lto_input && !sym.is_wrapped();
                match (in_ir, esym.is_undef()) {
                    (true, true) => LDPR_RESOLVED_IR,
                    (true, false) => LDPR_PREEMPTED_IR,
                    (false, true) => LDPR_RESOLVED_EXEC,
                    (false, false) => LDPR_PREEMPTED_REG,
                }
            }
        };
    }
    LDPS_OK
}

unsafe extern "C" fn get_api_version(
    _plugin_identifier: *const c_char,
    _plugin_version: c_uint,
    minimal_api_supported: c_int,
    maximal_api_supported: c_int,
    linker_identifier: *mut *const c_char,
    linker_version: *mut *const c_char,
) -> c_int {
    if LAPI_V1 < minimal_api_supported {
        fatal!(diag(), "LTO plugin does not support V0 or V1 API");
    }
    static LINKER_VERSION: OnceLock<CString> = OnceLock::new();
    *linker_identifier = c"mold".as_ptr();
    *linker_version = LINKER_VERSION
        .get_or_init(|| CString::new(VERSION).unwrap())
        .as_ptr();
    if LAPI_V1 <= maximal_api_supported {
        HOOKS.lock().unwrap().gcc_api_v1 = true;
        return LAPI_V1;
    }
    LAPI_V0
}

fn dlerror_string() -> String {
    // SAFETY: dlerror returns a static string or null.
    unsafe {
        let err = libc::dlerror();
        if err.is_null() {
            String::new()
        } else {
            CStr::from_ptr(err).to_string_lossy().into_owned()
        }
    }
}

/// Loads the plugin and hands it the transfer vector, once.
fn load_plugin<E: Arch>(ctx: &Context<E>) {
    LOADED.get_or_init(|| {
        DIAG.store(
            std::sync::Arc::as_ptr(&ctx.diag) as *mut Diagnostics,
            Ordering::Release,
        );

        let path = CString::new(ctx.args.plugin.as_str()).unwrap();
        // SAFETY: plain dlopen/dlsym calls.
        let onload: OnloadFn = unsafe {
            let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                fatal!(ctx, "could not open plugin file: {}", dlerror_string());
            }
            let onload = libc::dlsym(handle, c"onload".as_ptr());
            if onload.is_null() {
                fatal!(
                    ctx,
                    "failed to load plugin {}: {}",
                    ctx.args.plugin,
                    dlerror_string()
                );
            }
            std::mem::transmute::<*mut c_void, OnloadFn>(onload)
        };

        // Strings in the transfer vector must outlive the plugin.
        let cstr = |s: &str| CString::new(s).unwrap().into_raw() as *const c_void;
        let func = |f: usize| f as *const c_void;
        let output = if ctx.args.shared {
            LDPO_DYN
        } else if ctx.args.pie {
            LDPO_PIE
        } else {
            LDPO_EXEC
        };

        let mut tv = vec![
            TagValue::ptr(LDPT_MESSAGE, func(mold_lto_message as *const () as usize)),
            TagValue::int(LDPT_LINKER_OUTPUT, output),
        ];
        for opt in &ctx.args.plugin_opt {
            tv.push(TagValue::ptr(LDPT_OPTION, cstr(opt)));
        }
        tv.extend([
            TagValue::ptr(
                LDPT_REGISTER_CLAIM_FILE_HOOK,
                func(register_claim_file_hook as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK,
                func(register_all_symbols_read_hook as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_REGISTER_CLEANUP_HOOK,
                func(register_cleanup_hook as *const () as usize),
            ),
            TagValue::ptr(LDPT_ADD_SYMBOLS, func(add_symbols as *const () as usize)),
            TagValue::ptr(LDPT_GET_SYMBOLS, func(get_symbols_v1 as *const () as usize)),
            TagValue::ptr(
                LDPT_ADD_INPUT_FILE,
                func(add_input_file::<E> as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_FILE,
                func(get_input_file as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_RELEASE_INPUT_FILE,
                func(release_input_file as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_ADD_INPUT_LIBRARY,
                func(add_input_library as *const () as usize),
            ),
            TagValue::ptr(LDPT_OUTPUT_NAME, cstr(&ctx.args.output)),
            TagValue::ptr(
                LDPT_SET_EXTRA_LIBRARY_PATH,
                func(set_extra_library_path as *const () as usize),
            ),
            TagValue::ptr(LDPT_GET_VIEW, func(get_view as *const () as usize)),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_COUNT,
                func(get_input_section_count as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_TYPE,
                func(get_input_section_type as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_NAME,
                func(get_input_section_name as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_CONTENTS,
                func(get_input_section_contents as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_UPDATE_SECTION_ORDER,
                func(update_section_order as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_ALLOW_SECTION_ORDERING,
                func(allow_section_ordering as *const () as usize),
            ),
            TagValue::ptr(LDPT_ADD_SYMBOLS_V2, func(add_symbols as *const () as usize)),
            TagValue::ptr(
                LDPT_GET_SYMBOLS_V2,
                func(get_symbols_v2::<E> as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_ALLOW_UNIQUE_SEGMENT_FOR_SECTIONS,
                func(allow_unique_segment_for_sections as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_UNIQUE_SEGMENT_FOR_SECTIONS,
                func(unique_segment_for_sections as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_SYMBOLS_V3,
                func(get_symbols_v3::<E> as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_ALIGNMENT,
                func(get_input_section_alignment as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_INPUT_SECTION_SIZE,
                func(get_input_section_size as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_REGISTER_NEW_INPUT_HOOK,
                func(register_new_input_hook as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_WRAP_SYMBOLS,
                func(get_wrap_symbols as *const () as usize),
            ),
            TagValue::ptr(
                LDPT_GET_API_VERSION,
                func(get_api_version as *const () as usize),
            ),
            TagValue::int(LDPT_NULL, 0),
        ]);

        // SAFETY: the transfer vector is terminated by LDPT_NULL.
        let status = unsafe { onload(tv.as_ptr()) };
        if status != LDPS_OK {
            fatal!(ctx, "LTO plugin's onload failed: {status}");
        }
    });
}

/// Whether the plugin looks like LLVM's rather than GCC's.
fn is_llvm<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.args.plugin.contains("LLVMgold.")
}

/// Whether the plugin supports `get_symbols_v3`: any LLVM, and GCC 12 or
/// newer.
fn supports_v3_api<E: Arch>(ctx: &Context<E>) -> bool {
    HOOKS.lock().unwrap().gcc_api_v1 || is_llvm(ctx)
}

/// Describes a file to the plugin, which reads it through a descriptor,
/// at an offset for an archive member. The descriptor stays open as long
/// as the returned file does.
fn plugin_input_file<E: Arch>(
    ctx: &Context<E>,
    mf: &'static MappedFile,
) -> (PluginInputFile, File) {
    let container = mf.parent.unwrap_or(mf);
    let file = File::open(&container.name)
        .unwrap_or_else(|e| fatal!(ctx, "cannot open {}: {e}", container.name));
    let input = PluginInputFile {
        name: CString::new(container.name.as_str()).unwrap().into_raw(),
        fd: file.as_raw_fd(),
        offset: mf.offset() as u64,
        filesize: mf.size() as u64,
        handle: mf as *const MappedFile as *mut c_void,
    };
    (input, file)
}

/// Reads the symbols of an IR object through the plugin. Returns `None`
/// for an archive member the plugin declines.
pub fn read_lto_object<E: Arch>(
    ctx: &Context<E>,
    mf: &'static MappedFile,
    archive_name: String,
) -> Option<ObjectFile> {
    if ctx.args.plugin.is_empty() {
        fatal!(
            ctx,
            "{}: unable to handle this LTO object file because the -plugin option was not provided. \
             Please make sure you added -flto not only when creating object files but also when linking \
             the final executable.",
            mf.name
        );
    }
    load_plugin(ctx);
    let Some(claim_file) = HOOKS.lock().unwrap().claim_file else {
        fatal!(ctx, "LTO plugin did not register a claim_file hook");
    };

    // Input files are read in parallel, but claims are serialized: the
    // plugin returns a file's symbols through a global buffer.
    let _claim = CLAIM_LOCK.lock().unwrap();
    let (input, file) = plugin_input_file(ctx, mf);
    let mut claimed: c_int = 0;
    // SAFETY: `input` describes an open file.
    unsafe { claim_file(&input, &mut claimed) };
    drop(file);

    if claimed == 0 {
        if mf.parent.is_none() && mf.thin_parent.is_none() {
            fatal!(
                ctx,
                "{}: not claimed by the LTO plugin; please make sure you are using the same compiler of the \
                 same version for all object files",
                mf.name
            );
        }
        return None;
    }

    // A null symbol followed by the plugin's symbols, with their names in
    // a string table of our own.
    let symbols = std::mem::take(&mut *CLAIMED_SYMBOLS.lock().unwrap());
    let mut strtab = vec![0u8];
    let mut elf_syms = vec![ElfSym::default()];
    let mut comdat_keys = vec![None];
    for sym in &symbols {
        let mut esym = sym.to_elf_sym();
        esym.st_name = strtab.len() as u32;
        strtab.extend_from_slice(&sym.name);
        strtab.push(0);
        elf_syms.push(esym);
        comdat_keys.push(sym.comdat_key.as_ref().map(|key| leak_bytes(key.clone())));
    }
    Some(ObjectFile::lto_input::<E>(
        mf,
        archive_name,
        SymTable::from_records(RecordLayout::of::<E>(), &elf_syms),
        leak_bytes(strtab),
        comdat_keys,
    ))
}

/// Restarts the linker with the archive members that turned out to be
/// unneeded excluded, for plugins without `get_symbols_v3`, which
/// otherwise provides no way to withdraw a file that was claimed.
fn restart_process<E: Arch>(ctx: &Context<E>) -> ! {
    let mut args: Vec<String> = ctx.cmdline_args.clone();
    for file in &ctx.objs {
        if file.is_lto_input && !file.base.is_reachable() {
            args.push(format!(
                "--:ignore-ir-file={}",
                file.base.mf.unwrap().identifier()
            ));
        }
    }
    args.push("--:lto-pass2".to_string());

    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let err = std::process::Command::new(crate::util::self_path())
        .args(&args[1..])
        .exec();
    eprintln!("mold: execv failed: {err}");
    std::process::exit(1);
}

/// Has the plugin compile the IR objects. The resulting objects are added
/// to the context.
pub fn run_plugin<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("run_lto_plugin");
    load_plugin(ctx);

    if !ctx.args.lto_pass2 && !supports_v3_api(ctx) {
        restart_process(ctx);
    }

    // Definitions in IR objects that regular objects refer to must survive
    // the optimization.
    let referenced: Vec<SymbolId> = {
        let ctx: &Context<E> = ctx;
        ctx.objs
            .par_iter()
            .filter(|file| !file.is_lto_input)
            .flat_map_iter(|file| {
                file.base.global_symbols().iter().copied().filter(|&id| {
                    matches!(ctx.symbols[id].file(), Some(FileId::Obj(owner)) if ctx.objs[owner.index()].is_lto_input)
                })
            })
            .collect()
    };
    for id in referenced {
        ctx.symbols[id].set_referenced_by_regular_obj(true);
    }

    // Wrapped symbols are referred to by name from regular objects, and
    // -u symbols must be kept.
    let mut names: Vec<String> = Vec::new();
    for name in &ctx.args.wrap {
        names.extend([
            name.clone(),
            format!("__wrap_{name}"),
            format!("__real_{name}"),
        ]);
    }
    names.extend(ctx.args.undefined.iter().cloned());
    for name in names {
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].set_referenced_by_regular_obj(true);
    }

    // Objects with .gnu.offload_lto_.* sections carry code for
    // accelerators, which the backend needs as well.
    let claim_file = HOOKS
        .lock()
        .unwrap()
        .claim_file
        .expect("the plugin registered a claim_file hook");
    for fi in 0..ctx.objs.len() {
        let file = &ctx.objs[fi];
        if file.base.is_reachable() && !file.is_lto_input && file.is_gcc_offload_obj {
            let (input, _file) = plugin_input_file(ctx, file.base.mf.unwrap());
            let mut claimed: c_int = 0;
            // SAFETY: `input` describes an open file.
            unsafe { claim_file(&input, &mut claimed) };
        }
    }

    // The hook asks for symbol resolutions, compiles, and returns the
    // objects through add_input_file.
    let all_symbols_read = HOOKS
        .lock()
        .unwrap()
        .all_symbols_read
        .expect("the plugin registered an all_symbols_read hook");
    CONTEXT.store(ctx as *mut Context<E> as *mut c_void, Ordering::Release);
    // SAFETY: the callbacks are the only users of the context until the
    // hook returns.
    let status = unsafe { all_symbols_read() };
    CONTEXT.store(ptr::null_mut(), Ordering::Release);
    if status != LDPS_OK {
        fatal!(ctx, "LTO: all_symbols_read_hook returns {status}");
    }
}

/// Lets the plugin remove its temporary files.
pub fn cleanup() {
    let cleanup = HOOKS.lock().unwrap().cleanup;
    if let Some(cleanup) = cleanup {
        // SAFETY: a hook registered by the plugin.
        unsafe { cleanup() };
    }
}
