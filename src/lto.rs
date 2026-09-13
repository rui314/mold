//! This file handles the linker plugin to support LTO (Link-Time
//! Optimization).
//!
//! LTO is a technique to do whole-program optimization to a program. Since
//! a linker sees the whole program as opposed to a single compilation
//! unit, it in theory can do some optimizations that cannot be done in the
//! usual separate compilation model. For example, LTO should be able to
//! inline functions that are defined in other compilation unit.
//!
//! In GCC and Clang, all you have to do to enable LTO is adding the
//! `-flto` flag to the compiler and the linker command lines. If `-flto`
//! is given, the compiler generates a file that contains not machine code
//! but the compiler's IR (intermediate representation). In GCC, the output
//! is an ELF file which wraps GCC's IR. In LLVM, it's not even an ELF file
//! but just a raw LLVM IR file.
//!
//! Here is what we have to do if at least one input file is not a usual
//! ELF file but an IR object file:
//!
//!  1. Read symbols both from usual ELF files and from IR object files and
//!     resolve symbols as usual.
//!
//!  2. Pass all IR objects to the compiler backend. The compiler backend
//!     compiles the IRs and returns a few big ELF object files as a
//!     result.
//!
//!  3. Parse the returned ELF files and overwrite IR object symbols with
//!     the returned ones, discarding IR object files.
//!
//!  4. Continue the rest of the linking process as usual.
//!
//! When gcc or clang inovkes ld, they pass `-plugin /path/to/linker-plugin.so`
//! to the linker. The given .so file provides a way to call the compiler
//! backend.
//!
//! The linker plugin API is documented at
//! https://gcc.gnu.org/wiki/whopr/driver, though the document is a bit
//! outdated.
//!
//! Frankly, the linker plugin API is peculiar and is not very easy to use.
//! For some reason, the API functions don't return the result of a
//! function call as a return value but instead calls other function with
//! the result as its argument to "return" the result.
//!
//! For example, the first thing you need to do after dlopen()'ing a linker
//! plugin .so is to call `onload` function with a list of callback
//! functions. `onload` calls callbacks to notify about the pointers to
//! other functions the linker plugin provides. I don't know why `onload`
//! can't just return a list of functions or why the linker plugin can't
//! define not only `onload` but other functions, but that's what it is.
//!
//! Here is the steps to use the linker plugin:
//!
//!  1. dlopen() the linker plugin .so and call `onload` to obtain pointers
//!     to other functions provided by the plugin.
//!
//!  2. Call `claim_file_hook` with an IR object file to read its symbol
//!     table. `claim_file_hook` calls the `add_symbols` callback to
//!     "return" a list of symbols.
//!
//!  3. `claim_file_hook` returns LDPT_OK only when the plugin wants to
//!     handle a given file. Since we pass only IR object files to the
//!     plugin in mold, it always returns LDPT_OK in our case.
//!
//!  4. Once we made a decision as to which object file to include into the
//!     output file, we call `all_symbols_read_hook` to compile IR objects
//!     into a few big ELF files. That function calls the `get_symbols`
//!     callback to ask us about the symbol resolution results. (The
//!     compiler backend needs to know whether an undefined symbol in an IR
//!     object was resolved to a regular object file or a shared object to
//!     do whole program optimization, for example.)
//!
//!  5. `all_symbols_read_hook` "returns" the result by calling the
//!     `add_input_file` callback. The callback is called with a path to an
//!     LTO'ed ELF file. We parse that ELF file and override symbols
//!     defined by IR objects with the ELF file's ones.
//!
//!  6. Lastly, we call `cleanup_hook` to remove temporary files created by
//!     the compiler backend.
//!
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
#[cfg(not(windows))]
use std::os::unix::io::AsRawFd;
#[cfg(not(windows))]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

use rayon::prelude::*;

use crate::arch::Arch;
use crate::cmdline::VERSION;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, ObjId, ObjectFile, ObjectOrigin};
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
    #[cfg(not(windows))]
    fd: c_int,
    #[cfg(windows)]
    fd: *mut c_void,
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
    /// The printf-like diagnostics callback, defined in `c/lto-message.c`.
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
    fn to_elf_sym<E: Layout>(&self) -> ElfSym<E> {
        let mut esym = ElfSym::<E>::default();
        esym.st_size_mut().set(self.size);
        esym.st_shndx_mut().set(match self.def {
            LDPK_DEF | LDPK_WEAKDEF => SHN_ABS as u16,
            LDPK_COMMON => SHN_COMMON as u16,
            _ => SHN_UNDEF as u16,
        });
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

// Global variables
// We store LTO-related information to global variables,
// as the LTO plugin is not thread-safe by design anyway.
static LOADED: AtomicBool = AtomicBool::new(false);
static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    claim_file: None,
    all_symbols_read: None,
    cleanup: None,
    gcc_api_v1: false,
});

/// The plugin hands back the symbols of the file being claimed through
/// this buffer.
static CLAIMED_SYMBOLS: Mutex<Vec<ClaimedSymbol>> = Mutex::new(Vec::new());

/// The linker's state, for callbacks. `CONTEXT` is set only while the plugin
/// is compiling, when nothing else touches the context.
static CONTEXT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// Event handlers
/// Reports a message the plugin formatted.
///
/// # Safety
///
/// `msg` must point to a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mold_lto_report(level: c_int, msg: *const c_char) {
    let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    match level {
        LDPL_INFO => out!("{msg}"),
        LDPL_WARNING => warn!("{msg}"),
        _ => fatal!("{msg}"),
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
    let path = crate::util::os_str(CStr::from_ptr(path).to_bytes());
    let mf = must_open_file(std::path::Path::new(""), path);
    mf.set_dependency(false);

    let mut file = ObjectFile::<E>::new(mf, std::path::PathBuf::new());
    file.origin = ObjectOrigin::LtoOutput;
    file.base.set_reachable(true);
    file.base.priority = ctx.lto_file_priority;
    ctx.lto_file_priority += 1;
    // The corresponding C++ path resolves these immediately:
    // parse_symbols() only registers global symbols. Create their shared
    // Symbol objects and fill in the file's pointers before resolving.
    //
    // The Rust port gathers and resolves the registered symbols after the
    // plugin callback returns.
    file.register_global_symbols(&ctx.args, &mut ctx.symbol_bin());
    let id = ObjId(ctx.objs.push(Box::new(file)));
    ctx.file_by_priority.push(Some(FileId::Obj(id)));
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

/// get_symbols teaches the LTO plugin as to how we have resolved symbols.
/// The plugin uses the symbol resolution info to optimize the program.
///
/// For example, if a definition in an IR file is not referenced by
/// non-IR objects at all, the plugin may choose to completely inline
/// that definition within the IR objects and remove the symbol from the
/// LTO result. On the other hand, if a definition is referenced by a
/// non-IR object, it has to keep the symbol in the LTO result.
unsafe fn get_symbols<E: Arch>(
    handle: *const c_void,
    nsyms: c_int,
    psyms: *mut PluginSymbol,
    is_v2: bool,
) -> c_int {
    let ctx = &*(CONTEXT.load(Ordering::Acquire) as *const Context<E>);
    let psyms = std::slice::from_raw_parts_mut(psyms, nsyms as usize);
    let handle = handle as *const MappedFile;
    let Some(file) = ctx
        .objs
        .iter()
        .find(|f| f.base.mf.is_some_and(|mf| ptr::eq(mf, handle)))
    else {
        return LDPS_BAD_HANDLE;
    };

    // If file is an archive member which was not chose to be included in
    // to the final result, we need to make the plugin to ignore all
    // symbols.
    if !file.base.is_reachable() {
        for psym in psyms {
            psym.resolution = LDPR_PREEMPTED_REG;
        }
        return LDPS_NO_SYMS;
    }

    // Set the symbol resolution results to psyms.
    let this = FileId::Obj(file.id());
    for (i, psym) in psyms.iter_mut().enumerate() {
        let esym = &file.base.elf_syms[i + 1];
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
                let in_ir = ctx.objs[owner.index()].is_lto_input() && !sym.is_wrapped();
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
        fatal!("LTO plugin does not support V0 or V1 API");
    }
    // The plugin reads the string after this function has returned
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

#[cfg(windows)]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlerror() -> *mut c_char;
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

#[cfg(not(windows))]
unsafe fn dynamic_error() -> *mut c_char {
    unsafe { libc::dlerror() }
}

#[cfg(windows)]
unsafe fn dynamic_error() -> *mut c_char {
    unsafe { dlerror() }
}

#[cfg(not(windows))]
unsafe fn dynamic_open(path: *const c_char) -> *mut c_void {
    unsafe { libc::dlopen(path, libc::RTLD_NOW | libc::RTLD_LOCAL) }
}

#[cfg(windows)]
unsafe fn dynamic_open(path: *const c_char) -> *mut c_void {
    unsafe { dlopen(path, 0) }
}

#[cfg(not(windows))]
unsafe fn dynamic_symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { libc::dlsym(handle, name) }
}

#[cfg(windows)]
unsafe fn dynamic_symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { dlsym(handle, name) }
}

fn dlerror_string() -> String {
    // SAFETY: dlerror returns a static string or null.
    unsafe {
        let err = dynamic_error();
        if err.is_null() {
            String::new()
        } else {
            CStr::from_ptr(err).to_string_lossy().into_owned()
        }
    }
}

/// dlopen the linker plugin file
fn load_plugin<E: Arch>(ctx: &Context<E>) {
    // The file reader claims IR files serially in the command line order.
    if LOADED.swap(true, Ordering::Relaxed) {
        return;
    }
    let path = CString::new(ctx.args.plugin.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: plain dlopen/dlsym calls.
    let onload: OnloadFn = unsafe {
        let handle = dynamic_open(path.as_ptr());
        if handle.is_null() {
            fatal!("could not open plugin file: {}", dlerror_string());
        }
        let onload = dynamic_symbol(handle, c"onload".as_ptr());
        if onload.is_null() {
            fatal!(
                "failed to load plugin {}: {}",
                ctx.args.plugin.display(),
                dlerror_string()
            );
        }
        std::mem::transmute::<*mut c_void, OnloadFn>(onload)
    };

    // Strings in the transfer vector must outlive the plugin.
    let cstr = |s: &[u8]| CString::new(s).unwrap().into_raw() as *const c_void;
    let output = if ctx.args.shared {
        LDPO_DYN
    } else if ctx.args.pie {
        LDPO_PIE
    } else {
        LDPO_EXEC
    };

    let mut tv = vec![
        TagValue::ptr(LDPT_MESSAGE, mold_lto_message as *const c_void),
        TagValue::int(LDPT_LINKER_OUTPUT, output),
    ];
    for opt in &ctx.args.plugin_opt {
        tv.push(TagValue::ptr(LDPT_OPTION, cstr(opt)));
    }
    tv.extend([
        TagValue::ptr(
            LDPT_REGISTER_CLAIM_FILE_HOOK,
            register_claim_file_hook as *const c_void,
        ),
        TagValue::ptr(
            LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK,
            register_all_symbols_read_hook as *const c_void,
        ),
        TagValue::ptr(
            LDPT_REGISTER_CLEANUP_HOOK,
            register_cleanup_hook as *const c_void,
        ),
        TagValue::ptr(LDPT_ADD_SYMBOLS, add_symbols as *const c_void),
        TagValue::ptr(LDPT_GET_SYMBOLS, get_symbols_v1 as *const c_void),
        TagValue::ptr(LDPT_ADD_INPUT_FILE, add_input_file::<E> as *const c_void),
        TagValue::ptr(LDPT_GET_INPUT_FILE, get_input_file as *const c_void),
        TagValue::ptr(LDPT_RELEASE_INPUT_FILE, release_input_file as *const c_void),
        TagValue::ptr(LDPT_ADD_INPUT_LIBRARY, add_input_library as *const c_void),
        TagValue::ptr(
            LDPT_OUTPUT_NAME,
            CString::new(ctx.args.output.as_os_str().as_encoded_bytes())
                .unwrap()
                .into_raw() as *const c_void,
        ),
        TagValue::ptr(
            LDPT_SET_EXTRA_LIBRARY_PATH,
            set_extra_library_path as *const c_void,
        ),
        TagValue::ptr(LDPT_GET_VIEW, get_view as *const c_void),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_COUNT,
            get_input_section_count as *const c_void,
        ),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_TYPE,
            get_input_section_type as *const c_void,
        ),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_NAME,
            get_input_section_name as *const c_void,
        ),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_CONTENTS,
            get_input_section_contents as *const c_void,
        ),
        TagValue::ptr(
            LDPT_UPDATE_SECTION_ORDER,
            update_section_order as *const c_void,
        ),
        TagValue::ptr(
            LDPT_ALLOW_SECTION_ORDERING,
            allow_section_ordering as *const c_void,
        ),
        TagValue::ptr(LDPT_ADD_SYMBOLS_V2, add_symbols as *const c_void),
        TagValue::ptr(LDPT_GET_SYMBOLS_V2, get_symbols_v2::<E> as *const c_void),
        TagValue::ptr(
            LDPT_ALLOW_UNIQUE_SEGMENT_FOR_SECTIONS,
            allow_unique_segment_for_sections as *const c_void,
        ),
        TagValue::ptr(
            LDPT_UNIQUE_SEGMENT_FOR_SECTIONS,
            unique_segment_for_sections as *const c_void,
        ),
        TagValue::ptr(LDPT_GET_SYMBOLS_V3, get_symbols_v3::<E> as *const c_void),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_ALIGNMENT,
            get_input_section_alignment as *const c_void,
        ),
        TagValue::ptr(
            LDPT_GET_INPUT_SECTION_SIZE,
            get_input_section_size as *const c_void,
        ),
        TagValue::ptr(
            LDPT_REGISTER_NEW_INPUT_HOOK,
            register_new_input_hook as *const c_void,
        ),
        TagValue::ptr(LDPT_GET_WRAP_SYMBOLS, get_wrap_symbols as *const c_void),
        TagValue::ptr(LDPT_GET_API_VERSION, get_api_version as *const c_void),
        TagValue::int(LDPT_NULL, 0),
    ]);

    // SAFETY: the transfer vector is terminated by LDPT_NULL.
    let status = unsafe { onload(tv.as_ptr()) };
    if status != LDPS_OK {
        fatal!("LTO plugin's onload failed: {status}");
    }
}

/// Returns true if a given linker plugin looks like LLVM's one.
/// Returns false if it's GCC.
fn is_llvm<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.args
        .plugin
        .as_os_str()
        .as_encoded_bytes()
        .windows(9)
        .any(|s| s == b"LLVMgold.")
}

/// Returns true if a given linker plugin supports the get_symbols_v3 API.
/// Any version of LLVM and GCC 12 or newer support it.
fn supports_v3_api<E: Arch>(ctx: &Context<E>) -> bool {
    HOOKS.lock().unwrap().gcc_api_v1 || is_llvm(ctx)
}

/// Describes a file to the plugin, which reads it through a descriptor,
/// at an offset for an archive member. The descriptor stays open as long
/// as the returned file does.
fn plugin_input_file(mf: &'static MappedFile) -> (PluginInputFile, File) {
    let container = mf.parent.unwrap_or(mf);
    let file = File::open(&container.name)
        .unwrap_or_else(|e| fatal!("cannot open {}: {e}", container.name.display()));
    let input = PluginInputFile {
        name: CString::new(container.name.as_os_str().as_encoded_bytes()).unwrap().into_raw(),
        #[cfg(not(windows))]
        fd: file.as_raw_fd(),
        #[cfg(windows)]
        fd: file.as_raw_handle(),
        offset: mf.offset() as u64,
        filesize: mf.size() as u64,
        handle: mf as *const MappedFile as *mut c_void,
    };
    (input, file)
}

/// Reads the symbols of an IR object through the plugin. Returns `None`
/// for an archive member the plugin declines.
pub fn read_lto_object<E: Arch>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    archive_name: std::path::PathBuf,
) -> Option<ObjectFile<E>> {
    if ctx.args.plugin.as_os_str().is_empty() {
        fatal!("{}: unable to handle this LTO object file because the -plugin option was not provided. \
             Please make sure you added -flto not only when creating object files but also when linking \
             the final executable.",
            mf.name.display()
        );
    }
    load_plugin(ctx);
    let Some(claim_file) = HOOKS.lock().unwrap().claim_file else {
        fatal!("LTO plugin did not register a claim_file hook");
    };

    // Create plugin's object instance
    let (input, file) = plugin_input_file(mf);
    let mut claimed: c_int = 0;
    // claim_file_hook() calls add_symbols() which initializes `plugin_symbols`
    // SAFETY: `input` describes an open file.
    unsafe { claim_file(&input, &mut claimed) };
    drop(file);

    if claimed == 0 {
        if mf.parent.is_none() && mf.thin_parent.is_none() {
            fatal!("{}: not claimed by the LTO plugin; please make sure you are using the same compiler of the \
                 same version for all object files",
                mf.name.display()
            );
        }
        return None;
    }

    // Create a symbol strtab
    let symbols = std::mem::take(&mut *CLAIMED_SYMBOLS.lock().unwrap());
    let mut strtab = vec![0u8];
    // Initialize esyms
    let mut elf_syms = vec![ElfSym::<E>::default()];
    let mut comdat_keys = vec![None];
    for sym in &symbols {
        let mut esym = sym.to_elf_sym::<E>();
        esym.st_name_mut().set(strtab.len() as u32);
        strtab.extend_from_slice(&sym.name);
        strtab.push(0);
        elf_syms.push(esym);
        // comdat_key is non-null if the symbol is defined in a comdat member
        // section. We handle such symbols differently than comdat symbols in
        // a regular file because, unlike regular object files, IR files don't
        // have input sections.
        comdat_keys.push(sym.comdat_key.as_ref().map(|key| leak_bytes(key.clone())));
    }
    // Create mold's object instance
    Some(ObjectFile::<E>::lto_input(
        mf,
        archive_name,
        elf_syms,
        leak_bytes(strtab),
        comdat_keys,
    ))
}

/// This function restarts mold itself with `--:lto-pass2` and
/// `--:ignore-ir-file` flags. We do this as a workaround for the old
/// linker plugins that do not support the get_symbols_v3 API.
///
/// get_symbols_v1 and get_symbols_v2 don't provide a way to ignore an
/// object file we previously passed to the linker plugin. So we can't
/// "unload" object files in archives that we ended up not choosing to
/// include into the final output.
///
/// As a workaround, we restart the linker with a list of object files
/// the linker has to ignore, so that it won't read the object files
/// from archives next time.
///
/// This is an ugly hack and should be removed once GCC adopts the v3 API.
fn restart_process<E: Arch>(ctx: &Context<E>) -> ! {
    let mut args = ctx.cmdline_args.to_vec();
    for file in &ctx.objs {
        if file.is_lto_input() && !file.base.is_reachable() {
            let mut arg = std::ffi::OsString::from("--:ignore-ir-file=");
            arg.push(file.base.mf.unwrap().identifier());
            args.push(arg);
        }
    }
    args.push("--:lto-pass2".into());

    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let path = std::env::current_exe().expect("cannot get current executable path");
    #[cfg(not(windows))]
    let err = std::process::Command::new(path).args(&args[1..]).exec();
    #[cfg(windows)]
    let err = {
        let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let args: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(arg.as_encoded_bytes()).unwrap())
            .collect();
        let mut argv: Vec<*const c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
        argv.push(ptr::null());
        // SAFETY: path and every argument are NUL-terminated and argv ends in null.
        unsafe { libc::execv(path.as_ptr(), argv.as_ptr()) };
        std::io::Error::last_os_error()
    };
    eprintln!("mold: execv failed: {err}");
    std::process::exit(1);
}

// Entry point
/// Has the plugin compile the IR objects. The resulting objects are added
/// to the context.
pub fn run_plugin<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("run_lto_plugin");
    load_plugin(ctx);

    if !ctx.args.lto_pass2 && !supports_v3_api(ctx) {
        restart_process(ctx);
    }

    // Set `referenced_by_regular_obj` bit.
    let referenced: Vec<SymbolId> = {
        let ctx: &Context<E> = ctx;
        ctx.objs
            .par_iter()
            .filter(|file| !file.is_lto_input())
            .flat_map_iter(|file| {
                file.base.global_symbols().iter().copied().filter(|&id| {
                    matches!(ctx.symbols[id].file(), Some(FileId::Obj(owner)) if ctx.objs[owner.index()].is_lto_input())
                })
            })
            .collect()
    };
    for id in referenced {
        ctx.symbols[id].set_referenced_by_regular_obj(true);
    }

    // Symbols specified by the --wrap option needs to be visible from
    // regular object files.
    for name in &ctx.args.wrap {
        let id = ctx.symbols.get_or_intern(name);
        ctx.symbols[id].set_referenced_by_regular_obj(true);
        for prefix in [b"__wrap_", b"__real_"] {
            let id = ctx
                .symbols
                .get_or_intern(&[prefix, name.as_slice()].concat());
            ctx.symbols[id].set_referenced_by_regular_obj(true);
        }
    }
    // Keep some symbols
    for name in &ctx.args.undefined {
        let id = ctx.symbols.get_or_intern(name);
        ctx.symbols[id].set_referenced_by_regular_obj(true);
    }

    // Object files containing .gnu.offload_lto_.* sections need to be
    // given to the LTO backend. Such sections contains code and data for
    // peripherails (typically GPUs).
    let claim_file = HOOKS
        .lock()
        .unwrap()
        .claim_file
        .expect("the plugin registered a claim_file hook");
    for file in &ctx.objs {
        if file.base.is_reachable() && !file.is_lto_input() && file.is_gcc_offload_obj {
            let (input, _file) = plugin_input_file(file.base.mf.unwrap());
            let mut claimed: c_int = 0;
            // SAFETY: `input` describes an open file.
            unsafe { claim_file(&input, &mut claimed) };
        }
    }

    // all_symbols_read_hook() calls add_input_file() and add_input_library()
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
        fatal!("LTO: all_symbols_read_hook returns {status}");
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
