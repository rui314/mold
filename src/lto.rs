//! Link-time optimization via libLTO.
//!
//! With -flto, clang emits object files that are LLVM bitcode rather
//! than Mach-O. The linker is expected to load LLVM's libLTO
//! (clang passes its path as -lto_library), register every bitcode
//! module, tell the library which symbols must survive, and compile
//! them all into one Mach-O object that then joins the link like any
//! other input.

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::fatal;

// Symbol attribute bits from llvm-c/lto.h
pub const LTO_SYMBOL_DEFINITION_MASK: u32 = 0x700;
pub const LTO_SYMBOL_DEFINITION_REGULAR: u32 = 0x100;
pub const LTO_SYMBOL_DEFINITION_TENTATIVE: u32 = 0x200;
pub const LTO_SYMBOL_DEFINITION_WEAK: u32 = 0x300;
pub const LTO_SYMBOL_SCOPE_MASK: u32 = 0x3800;
pub const LTO_SYMBOL_SCOPE_INTERNAL: u32 = 0x800;
pub const LTO_SYMBOL_SCOPE_HIDDEN: u32 = 0x1000;

pub const LTO_CODEGEN_PIC_MODEL_DYNAMIC: u32 = 1;

/// The subset of libLTO's C API the linker uses, resolved with dlsym.
#[derive(Clone, Copy)]
pub struct Plugin {
    pub get_error_message: unsafe extern "C" fn() -> *const c_char,
    pub module_create_from_memory_with_path:
        unsafe extern "C" fn(*const c_void, usize, *const c_char) -> *mut c_void,
    pub module_dispose: unsafe extern "C" fn(*mut c_void),
    pub module_get_num_symbols: unsafe extern "C" fn(*mut c_void) -> u32,
    pub module_get_symbol_name: unsafe extern "C" fn(*mut c_void, u32) -> *const c_char,
    pub module_get_symbol_attribute: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    pub module_get_target_triple: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    pub codegen_create: unsafe extern "C" fn() -> *mut c_void,
    pub codegen_add_module: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    pub codegen_set_pic_model: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    pub codegen_add_must_preserve_symbol: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub codegen_set_cpu: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub codegen_set_should_embed_uselists: unsafe extern "C" fn(*mut c_void, bool),
    pub codegen_write_merged_modules: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    pub codegen_optimize: unsafe extern "C" fn(*mut c_void) -> bool,
    pub codegen_compile_optimized: unsafe extern "C" fn(*mut c_void, *mut usize) -> *const c_void,
}

impl Plugin {
    pub fn error_message(&self) -> String {
        // SAFETY: libLTO returns a NUL-terminated string or null.
        unsafe {
            let msg = (self.get_error_message)();
            if msg.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(msg).to_string_lossy().into_owned()
            }
        }
    }
}

/// Resolves one libLTO entry point as the function pointer type the
/// caller expects.
///
/// # Safety
///
/// `handle` must be a live dlopen handle and `T` the C signature of the
/// named function.
unsafe fn dlsym<T>(handle: *mut c_void, name: &CStr) -> T {
    // SAFETY: dlsym with a valid handle and a NUL-terminated name.
    let sym = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if sym.is_null() {
        fatal!("libLTO does not provide {}", name.to_string_lossy());
    }
    assert_eq!(size_of::<T>(), size_of::<*mut c_void>());
    // SAFETY: T is a function pointer type of the same size as the
    // symbol address, per the caller's contract.
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&sym) }
}

/// The host's name for LLVM's LTO library, when -lto_library does not
/// say: clang's macOS toolchains ship libLTO.dylib, Linux ones libLTO.so.
const DEFAULT_LTO_LIBRARY: &str =
    if cfg!(target_os = "macos") { "libLTO.dylib" } else { "libLTO.so" };

/// Loads libLTO from the given path (from -lto_library, with a plain
/// library-name fallback that relies on the dynamic loader's search).
pub fn load_plugin(path: Option<&Path>) -> Plugin {
    let path = CString::new(path.map_or(DEFAULT_LTO_LIBRARY.as_bytes(), crate::util::path_bytes))
        .unwrap_or_else(|_| fatal!("-lto_library: path contains a NUL byte"));
    // SAFETY: dlopen/dlsym with valid NUL-terminated strings.
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            fatal!(
                "could not load the LTO library {}; is -lto_library missing?",
                path.to_string_lossy()
            );
        }

        Plugin {
            get_error_message: dlsym(handle, c"lto_get_error_message"),
            module_create_from_memory_with_path: dlsym(
                handle,
                c"lto_module_create_from_memory_with_path",
            ),
            module_dispose: dlsym(handle, c"lto_module_dispose"),
            module_get_num_symbols: dlsym(handle, c"lto_module_get_num_symbols"),
            module_get_symbol_name: dlsym(handle, c"lto_module_get_symbol_name"),
            module_get_symbol_attribute: dlsym(handle, c"lto_module_get_symbol_attribute"),
            module_get_target_triple: dlsym(handle, c"lto_module_get_target_triple"),
            codegen_create: dlsym(handle, c"lto_codegen_create"),
            codegen_add_module: dlsym(handle, c"lto_codegen_add_module"),
            codegen_set_pic_model: dlsym(handle, c"lto_codegen_set_pic_model"),
            codegen_add_must_preserve_symbol: dlsym(
                handle,
                c"lto_codegen_add_must_preserve_symbol",
            ),
            codegen_set_cpu: dlsym(handle, c"lto_codegen_set_cpu"),
            codegen_set_should_embed_uselists: dlsym(
                handle,
                c"lto_codegen_set_should_embed_uselists",
            ),
            codegen_write_merged_modules: dlsym(handle, c"lto_codegen_write_merged_modules"),
            codegen_optimize: dlsym(handle, c"lto_codegen_optimize"),
            codegen_compile_optimized: dlsym(handle, c"lto_codegen_compile_optimized"),
        }
    }
}

/// What the command line asks of libLTO's code generator: the CPU to
/// compile for (-mcpu), and the output, if the intermediate files are
/// to be kept beside it (-save-temps).
pub struct CodegenOptions<'a> {
    pub cpu: Option<&'a str>,
    pub save_temps: Option<&'a Path>,
}

/// Optimizes the modules added to the code generator and compiles them
/// into one Mach-O object, as ld64 drives libLTO. -save-temps keeps the
/// merged bitcode before and after optimization and the object beside
/// the output, as <output>.lto.bc, .lto.opt.bc and .lto.o.
///
/// # Safety
///
/// `cg` must be a live code generator of the plugin's library.
pub unsafe fn compile(plugin: &Plugin, cg: *mut c_void, opts: &CodegenOptions) -> Vec<u8> {
    let temp_path = |suffix: &str| {
        let mut path = opts.save_temps?.as_os_str().to_owned();
        path.push(suffix);
        Some(path)
    };
    // SAFETY: the generator is live per the caller, and the strings
    // passed are NUL-terminated.
    unsafe {
        let save_bitcode = |suffix: &str| {
            if let Some(path) = temp_path(suffix)
                && let Ok(path) = CString::new(path.as_bytes())
            {
                (plugin.codegen_set_should_embed_uselists)(cg, true);
                (plugin.codegen_write_merged_modules)(cg, path.as_ptr());
            }
        };
        if let Some(cpu) = opts.cpu
            && let Ok(cpu) = CString::new(cpu)
        {
            (plugin.codegen_set_cpu)(cg, cpu.as_ptr());
        }
        save_bitcode(".lto.bc");
        if (plugin.codegen_optimize)(cg) {
            fatal!("lto_codegen_optimize failed: {}", plugin.error_message());
        }
        save_bitcode(".lto.opt.bc");
        let mut size = 0usize;
        let ptr = (plugin.codegen_compile_optimized)(cg, &raw mut size);
        if ptr.is_null() {
            fatal!("lto_codegen_compile_optimized failed: {}", plugin.error_message());
        }
        let data = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
        // ld64 keeps the object if it can, saying nothing otherwise.
        if let Some(path) = temp_path(".lto.o") {
            let _ = std::fs::write(path, &data);
        }
        data
    }
}

/// Writes the modules added to the code generator, merged into one, to
/// a bitcode file at `path` - what ld-prime makes of a -r link of
/// bitcode alone, so that the final link still optimizes it as a
/// whole. libLTO internalizes what it was not told to preserve, but
/// optimizes nothing. On failure, returns libLTO's message.
///
/// # Safety
///
/// `cg` must be a live code generator of the plugin's library.
pub unsafe fn write_merged_modules(
    plugin: &Plugin,
    cg: *mut c_void,
    path: &Path,
) -> Result<(), String> {
    let path = CString::new(crate::util::path_bytes(path))
        .map_err(|_| "output path contains a NUL byte".to_string())?;
    // SAFETY: the generator is live per the caller, and the path is
    // NUL-terminated.
    if unsafe { (plugin.codegen_write_merged_modules)(cg, path.as_ptr()) } {
        return Err(plugin.error_message());
    }
    Ok(())
}

/// A parsed bitcode module's symbol, in linker terms.
pub struct LtoSymbol {
    pub name: String,
    pub is_defined: bool,
    pub is_weak_def: bool,
    pub is_extern: bool,
    pub is_private_extern: bool,
}

/// Creates a module from a bitcode buffer.
fn create_module(plugin: &Plugin, data: &[u8], name: &Path) -> *mut c_void {
    let cname = CString::new(crate::util::path_bytes(name)).unwrap_or_default();
    // SAFETY: the buffer is valid for the call's duration; libLTO copies
    // what it needs.
    let module = unsafe {
        (plugin.module_create_from_memory_with_path)(
            data.as_ptr().cast::<c_void>(),
            data.len(),
            cname.as_ptr(),
        )
    };
    if module.is_null() {
        fatal!("{}: lto_module_create failed: {}", name.display(), plugin.error_message());
    }
    module
}

/// The target triple a bitcode file was compiled for, such as
/// arm64-apple-macosx13.0.0.
pub fn target_triple(plugin: &Plugin, data: &[u8], name: &Path) -> String {
    let module = create_module(plugin, data, name);
    // SAFETY: the module handle is valid until disposed of, and the
    // triple is a NUL-terminated string it owns.
    unsafe {
        let triple = CStr::from_ptr((plugin.module_get_target_triple)(module));
        let triple = triple.to_string_lossy().into_owned();
        (plugin.module_dispose)(module);
        triple
    }
}

/// Creates a module from a bitcode buffer and lists its symbols.
pub fn parse_module(plugin: &Plugin, data: &[u8], name: &Path) -> (usize, Vec<LtoSymbol>) {
    let module = create_module(plugin, data, name);
    let mut syms = Vec::new();
    // SAFETY: the module handle is valid; indices are in range.
    unsafe {
        let n = (plugin.module_get_num_symbols)(module);
        for i in 0..n {
            let cstr = CStr::from_ptr((plugin.module_get_symbol_name)(module, i));
            let attr = (plugin.module_get_symbol_attribute)(module, i);
            let def = attr & LTO_SYMBOL_DEFINITION_MASK;
            let scope = attr & LTO_SYMBOL_SCOPE_MASK;
            syms.push(LtoSymbol {
                name: cstr.to_string_lossy().into_owned(),
                is_defined: matches!(
                    def,
                    LTO_SYMBOL_DEFINITION_REGULAR
                        | LTO_SYMBOL_DEFINITION_TENTATIVE
                        | LTO_SYMBOL_DEFINITION_WEAK
                ),
                is_weak_def: def == LTO_SYMBOL_DEFINITION_WEAK,
                is_extern: scope != LTO_SYMBOL_SCOPE_INTERNAL && scope != 0,
                is_private_extern: scope == LTO_SYMBOL_SCOPE_HIDDEN,
            });
        }
    }
    (module as usize, syms)
}
