//! Link-time optimization via libLTO.
//!
//! With -flto, clang emits object files that are LLVM bitcode rather
//! than Mach-O. The linker is expected to load LLVM's libLTO
//! (clang passes its path as -lto_library), register every bitcode
//! module, tell the library which symbols must survive, and have it
//! compile them to Mach-O objects that then join the link like any
//! other input: the modules clang built for ThinLTO (-flto=thin) one
//! object each, through libLTO's thinlto_* API, and the rest merged
//! into one module and one object.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
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
    pub codegen_debug_options_array: unsafe extern "C" fn(*mut c_void, *const *const c_char, c_int),
    pub codegen_set_should_embed_uselists: unsafe extern "C" fn(*mut c_void, bool),
    pub codegen_write_merged_modules: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    pub codegen_optimize: unsafe extern "C" fn(*mut c_void) -> bool,
    pub codegen_compile_optimized: unsafe extern "C" fn(*mut c_void, *mut usize) -> *const c_void,
    pub get_version: unsafe extern "C" fn() -> *const c_char,
    pub module_is_thinlto: unsafe extern "C" fn(*mut c_void) -> bool,
    pub thinlto_debug_options: unsafe extern "C" fn(*const *const c_char, c_int),
    pub thinlto_create_codegen: unsafe extern "C" fn() -> *mut c_void,
    pub thinlto_codegen_set_pic_model: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    pub thinlto_codegen_set_cpu: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub thinlto_codegen_add_must_preserve_symbol:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int),
    pub thinlto_codegen_add_cross_referenced_symbol:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int),
    pub thinlto_codegen_add_module:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_int),
    pub thinlto_set_generated_objects_dir: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub thinlto_codegen_set_savetemps_dir: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub thinlto_codegen_set_cache_dir: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub thinlto_codegen_set_cache_pruning_interval: unsafe extern "C" fn(*mut c_void, c_int),
    pub thinlto_codegen_set_cache_entry_expiration: unsafe extern "C" fn(*mut c_void, u32),
    pub thinlto_codegen_set_final_cache_size_relative_to_available_space:
        unsafe extern "C" fn(*mut c_void, u32),
    pub thinlto_codegen_set_codegen_only: unsafe extern "C" fn(*mut c_void, bool),
    pub thinlto_codegen_process: unsafe extern "C" fn(*mut c_void),
    pub thinlto_module_get_num_objects: unsafe extern "C" fn(*mut c_void) -> u32,
    pub thinlto_module_get_object: unsafe extern "C" fn(*mut c_void, u32) -> ObjectBuffer,
    pub thinlto_module_get_num_object_files: unsafe extern "C" fn(*mut c_void) -> u32,
    pub thinlto_module_get_object_file: unsafe extern "C" fn(*mut c_void, u32) -> *const c_char,
}

/// libLTO's LTOObjectBuffer: an object ThinLTO compiled in memory.
#[repr(C)]
pub struct ObjectBuffer {
    pub buffer: *const c_char,
    pub size: usize,
}

impl Plugin {
    pub fn error_message(&self) -> String {
        // SAFETY: libLTO returns a NUL-terminated string or null.
        unsafe { c_string(self.get_error_message).unwrap_or_else(|| "unknown error".to_string()) }
    }

    /// The library's version, as ld-prime quotes it when LTO fails.
    pub fn version(&self) -> String {
        // SAFETY: as for error_message.
        unsafe { c_string(self.get_version).unwrap_or_default() }
    }
}

/// The string a libLTO function returns, if any.
///
/// # Safety
///
/// `f` must return a NUL-terminated string or null.
unsafe fn c_string(f: unsafe extern "C" fn() -> *const c_char) -> Option<String> {
    // SAFETY: per the caller's contract.
    unsafe {
        let s = f();
        (!s.is_null()).then(|| CStr::from_ptr(s).to_string_lossy().into_owned())
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

/// The LTO library of the toolchain the linker is installed in, if
/// there is one: ld-prime links the libLTO in lib beside its bin
/// directory (@rpath, which is @executable_path/../lib), as ld64 looked
/// for it by its own real path, and clang passes -lto_library with the
/// one beside it in the same way.
fn toolchain_lto_library() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let path = exe.parent()?.parent()?.join("lib").join(DEFAULT_LTO_LIBRARY);
    path.is_file().then_some(path)
}

/// Loads libLTO from the given path (from -lto_library), or else the
/// linker's toolchain's, or else the one the dynamic loader finds by
/// name in its search path.
pub fn load_plugin(path: Option<&Path>) -> Plugin {
    let default = path.is_none().then(toolchain_lto_library).flatten();
    let path = path.or(default.as_deref());
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
            codegen_debug_options_array: dlsym(handle, c"lto_codegen_debug_options_array"),
            codegen_set_should_embed_uselists: dlsym(
                handle,
                c"lto_codegen_set_should_embed_uselists",
            ),
            codegen_write_merged_modules: dlsym(handle, c"lto_codegen_write_merged_modules"),
            codegen_optimize: dlsym(handle, c"lto_codegen_optimize"),
            codegen_compile_optimized: dlsym(handle, c"lto_codegen_compile_optimized"),
            get_version: dlsym(handle, c"lto_get_version"),
            module_is_thinlto: dlsym(handle, c"lto_module_is_thinlto"),
            thinlto_debug_options: dlsym(handle, c"thinlto_debug_options"),
            thinlto_create_codegen: dlsym(handle, c"thinlto_create_codegen"),
            thinlto_codegen_set_pic_model: dlsym(handle, c"thinlto_codegen_set_pic_model"),
            thinlto_codegen_set_cpu: dlsym(handle, c"thinlto_codegen_set_cpu"),
            thinlto_codegen_add_must_preserve_symbol: dlsym(
                handle,
                c"thinlto_codegen_add_must_preserve_symbol",
            ),
            thinlto_codegen_add_cross_referenced_symbol: dlsym(
                handle,
                c"thinlto_codegen_add_cross_referenced_symbol",
            ),
            thinlto_codegen_add_module: dlsym(handle, c"thinlto_codegen_add_module"),
            thinlto_set_generated_objects_dir: dlsym(handle, c"thinlto_set_generated_objects_dir"),
            thinlto_codegen_set_savetemps_dir: dlsym(handle, c"thinlto_codegen_set_savetemps_dir"),
            thinlto_codegen_set_cache_dir: dlsym(handle, c"thinlto_codegen_set_cache_dir"),
            thinlto_codegen_set_cache_pruning_interval: dlsym(
                handle,
                c"thinlto_codegen_set_cache_pruning_interval",
            ),
            thinlto_codegen_set_cache_entry_expiration: dlsym(
                handle,
                c"thinlto_codegen_set_cache_entry_expiration",
            ),
            thinlto_codegen_set_final_cache_size_relative_to_available_space: dlsym(
                handle,
                c"thinlto_codegen_set_final_cache_size_relative_to_available_space",
            ),
            thinlto_codegen_set_codegen_only: dlsym(handle, c"thinlto_codegen_set_codegen_only"),
            thinlto_codegen_process: dlsym(handle, c"thinlto_codegen_process"),
            thinlto_module_get_num_objects: dlsym(handle, c"thinlto_module_get_num_objects"),
            thinlto_module_get_object: dlsym(handle, c"thinlto_module_get_object"),
            thinlto_module_get_num_object_files: dlsym(
                handle,
                c"thinlto_module_get_num_object_files",
            ),
            thinlto_module_get_object_file: dlsym(handle, c"thinlto_module_get_object_file"),
        }
    }
}

/// Hands the code generator the -mllvm options, which libLTO parses as
/// LLVM's command line when it optimizes (an unknown one ends the
/// process there, as it ends ld-prime).
///
/// # Safety
///
/// `cg` must be a live code generator of the plugin's library.
pub unsafe fn set_debug_options(plugin: &Plugin, cg: *mut c_void, options: &[Vec<u8>]) {
    if options.is_empty() {
        return;
    }
    let options: Vec<CString> =
        options.iter().map(|o| CString::new(o.as_slice()).unwrap_or_default()).collect();
    let ptrs: Vec<*const c_char> = options.iter().map(|o| o.as_ptr()).collect();
    // SAFETY: the generator is live per the caller; libLTO copies the
    // strings.
    unsafe { (plugin.codegen_debug_options_array)(cg, ptrs.as_ptr(), ptrs.len() as c_int) };
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
    let temp_path = |suffix: &str| Some(temp_path(opts.save_temps?, suffix));
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

/// A bitcode module for ThinLTO: the name that identifies it to libLTO,
/// unique in the link, and its bitcode.
pub struct ThinModule<'a> {
    pub id: CString,
    pub data: &'a [u8],
}

/// What the command line asks of ThinLTO.
pub struct ThinOptions<'a> {
    /// -mllvm: LLVM options, which ThinLTO takes globally.
    pub debug_options: &'a [Vec<u8>],
    /// -mcpu: the CPU to compile for.
    pub cpu: Option<&'a str>,
    /// -object_path_lto: the directory libLTO writes the objects to,
    /// for the debugger.
    pub objects_dir: Option<&'a Path>,
    /// -cache_path_lto and its policy.
    pub cache: Option<CacheOptions<'a>>,
    /// The output, if -save-temps keeps the intermediate files beside
    /// it.
    pub save_temps: Option<&'a Path>,
    /// -flto-codegen-only: compile the modules without optimizing.
    pub codegen_only: bool,
}

/// Where ThinLTO caches the objects it compiles, keyed by everything
/// that goes into one, and how it prunes them (0 for libLTO's default).
pub struct CacheOptions<'a> {
    pub dir: &'a Path,
    pub prune_interval: Option<i32>,
    pub expiration: u32,
    pub max_size: u32,
}

/// Hands ThinLTO the cache directory, which ld-prime creates (one level
/// of it, owner-only) if it is not one yet - or warns and goes without.
///
/// # Safety
///
/// `cg` must be a live ThinLTO code generator.
unsafe fn set_cache(plugin: &Plugin, cg: *mut c_void, cache: &CacheOptions) {
    use std::os::unix::fs::DirBuilderExt;
    if !cache.dir.is_dir()
        && let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(cache.dir)
    {
        let errno = e.raw_os_error().unwrap_or(0);
        crate::warn!("unable to create ThinLTO cache directory: {} ({errno})", cache.dir.display());
        return;
    }
    let dir = CString::new(crate::util::path_bytes(cache.dir)).unwrap_or_default();
    // SAFETY: the generator is live per the caller; the path is
    // NUL-terminated.
    unsafe {
        (plugin.thinlto_codegen_set_cache_dir)(cg, dir.as_ptr());
        if let Some(interval) = cache.prune_interval {
            (plugin.thinlto_codegen_set_cache_pruning_interval)(cg, interval);
        }
        (plugin.thinlto_codegen_set_cache_entry_expiration)(cg, cache.expiration);
        (plugin.thinlto_codegen_set_final_cache_size_relative_to_available_space)(
            cg,
            cache.max_size,
        );
    }
}

/// An object ThinLTO compiled a module to: in memory, or written to a
/// file in the -object_path_lto directory, which libLTO names.
pub struct ThinObject {
    pub path: Option<std::path::PathBuf>,
    pub data: Vec<u8>,
}

/// Compiles bitcode modules with ThinLTO, which optimizes each on its
/// own with what a summary of all of them says (importing functions
/// across modules to inline), into an object per module, as ld-prime
/// drives libLTO. `preserve` are the symbols the rest of the link
/// needs; `cross` those the modules reference (libLTO keeps both).
///
/// # Safety
///
/// The plugin must be a loaded libLTO.
pub unsafe fn compile_thin(
    plugin: &Plugin,
    modules: &[ThinModule],
    preserve: &[&str],
    cross: &[&str],
    opts: &ThinOptions,
) -> Vec<ThinObject> {
    let c = |s: &[u8]| CString::new(s).unwrap_or_default();
    // SAFETY: libLTO calls on a code generator created here, with
    // NUL-terminated strings and buffers that outlive the calls.
    unsafe {
        // LLVM's options are global; ThinLTO parses them up front.
        if !opts.debug_options.is_empty() {
            let options: Vec<CString> = opts.debug_options.iter().map(|o| c(o)).collect();
            let ptrs: Vec<*const c_char> = options.iter().map(|o| o.as_ptr()).collect();
            (plugin.thinlto_debug_options)(ptrs.as_ptr(), ptrs.len() as c_int);
        }
        let cg = (plugin.thinlto_create_codegen)();
        if cg.is_null() {
            fatal!("thinlto_create_codegen failed: {}", plugin.error_message());
        }
        if let Some(cache) = &opts.cache {
            set_cache(plugin, cg, cache);
        }
        if let Some(cpu) = opts.cpu {
            (plugin.thinlto_codegen_set_cpu)(cg, c(cpu.as_bytes()).as_ptr());
        }
        if (plugin.thinlto_codegen_set_pic_model)(cg, LTO_CODEGEN_PIC_MODEL_DYNAMIC) {
            fatal!("could not set codegen model: {}", plugin.error_message());
        }
        for name in preserve {
            (plugin.thinlto_codegen_add_must_preserve_symbol)(
                cg,
                name.as_ptr().cast(),
                name.len() as c_int,
            );
        }
        for name in cross {
            (plugin.thinlto_codegen_add_cross_referenced_symbol)(
                cg,
                name.as_ptr().cast(),
                name.len() as c_int,
            );
        }
        for module in modules {
            (plugin.thinlto_codegen_add_module)(
                cg,
                module.id.as_ptr(),
                module.data.as_ptr().cast(),
                module.data.len() as c_int,
            );
        }
        if let Some(output) = opts.save_temps {
            make_save_temps_dir(output);
            let dir = temp_path(output, ".thinlto.bcs/");
            (plugin.thinlto_codegen_set_savetemps_dir)(cg, c(dir.as_bytes()).as_ptr());
        }
        let objects_dir = opts.objects_dir.map(|dir| c(crate::util::path_bytes(dir)));
        if let Some(dir) = &objects_dir {
            (plugin.thinlto_set_generated_objects_dir)(cg, dir.as_ptr());
        }
        if opts.codegen_only {
            (plugin.thinlto_codegen_set_codegen_only)(cg, true);
        }
        (plugin.thinlto_codegen_process)(cg);

        let objects = match objects_dir {
            Some(_) => thin_object_files(plugin, cg),
            None => thin_object_buffers(plugin, cg),
        };
        if objects.is_empty() {
            fatal!(
                "could not do ThinLTO codegen (thinlto_codegen_process didn't produce any \
                 object): '{}', using libLTO version '{}'",
                plugin.error_message(),
                plugin.version()
            );
        }
        if let Some(output) = opts.save_temps {
            save_thin_objects(output, &objects);
        }
        objects
    }
}

/// A file -save-temps keeps beside the output: its name plus a suffix.
fn temp_path(output: &Path, suffix: &str) -> std::ffi::OsString {
    let mut path = output.as_os_str().to_owned();
    path.push(suffix);
    path
}

/// Has -save-temps keep ThinLTO's bitcode at each stage in
/// <output>.thinlto.bcs, which libLTO fills (and which ld-prime makes,
/// owner-only, if it is not a directory yet).
fn make_save_temps_dir(output: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    let dir = temp_path(output, ".thinlto.bcs/");
    if !Path::new(&dir).is_dir() && std::fs::DirBuilder::new().mode(0o700).create(&dir).is_err() {
        crate::warn!(
            "unable to create ThinLTO output directory for temporary bitcode files: {}",
            dir.display()
        );
    }
}

/// Keeps the objects ThinLTO compiled beside the output, for
/// -save-temps, as <output>.<index>.thinlto.o.
fn save_thin_objects(output: &Path, objects: &[ThinObject]) {
    for (i, obj) in objects.iter().enumerate() {
        let path = temp_path(output, &format!(".{i}.thinlto.o"));
        if std::fs::write(&path, &obj.data).is_err() {
            crate::warn!("unable to write temporary ThinLTO output: {}", path.display());
        }
    }
}

/// The objects ThinLTO compiled in memory. ld-prime skips an empty one.
///
/// # Safety
///
/// `cg` must be a ThinLTO code generator that has run.
unsafe fn thin_object_buffers(plugin: &Plugin, cg: *mut c_void) -> Vec<ThinObject> {
    let mut objects = Vec::new();
    // SAFETY: the indices are in range, and each buffer lives as long
    // as the generator.
    unsafe {
        for i in 0..(plugin.thinlto_module_get_num_objects)(cg) {
            let buf = (plugin.thinlto_module_get_object)(cg, i);
            if buf.size == 0 {
                crate::warn!("Ignoring empty buffer generated by ThinLTO");
                continue;
            }
            let data = std::slice::from_raw_parts(buf.buffer.cast::<u8>(), buf.size).to_vec();
            objects.push(ThinObject { path: None, data });
        }
    }
    objects
}

/// The objects ThinLTO wrote to the -object_path_lto directory, by the
/// paths libLTO gave them: <dir>/<index>.<arch>.thinlto.o.
///
/// # Safety
///
/// `cg` must be a ThinLTO code generator that has run with a directory
/// for its objects.
unsafe fn thin_object_files(plugin: &Plugin, cg: *mut c_void) -> Vec<ThinObject> {
    let mut objects = Vec::new();
    // SAFETY: the indices are in range, and each path is NUL-terminated.
    unsafe {
        for i in 0..(plugin.thinlto_module_get_num_object_files)(cg) {
            let path = CStr::from_ptr((plugin.thinlto_module_get_object_file)(cg, i));
            let path = std::path::PathBuf::from(crate::util::os_str(path.to_bytes()));
            let data = std::fs::read(&path)
                .unwrap_or_else(|e| fatal!("cannot read ThinLTO object {}: {}", path.display(), e));
            objects.push(ThinObject { path: Some(path), data });
        }
    }
    objects
}

/// A bitcode file registered for LTO.
pub struct BitcodeModule {
    /// The placeholder object that claims the module's symbols until
    /// LTO compiles it.
    pub obj: usize,
    /// libLTO's lto_module_t.
    pub handle: usize,
    /// The names the module defines, internal ones included (which the
    /// placeholder doesn't claim).
    pub defined: Vec<&'static str>,
    /// Whether clang built the module for ThinLTO (-flto=thin).
    pub is_thin: bool,
}

/// A bitcode file LTO compiled, as the passes after LTO still see it
/// once its placeholder object is retired.
pub struct LtoInput {
    /// The placeholder object.
    pub obj: usize,
    /// The external symbols the file defines, other than weakly.
    pub strong_defs: Vec<crate::symbol::SymbolId>,
    /// The names the module defined, internal ones included.
    pub defined: Vec<&'static str>,
}

/// The bitcode file each symbol of the object LTO compiled comes from,
/// by name, as ld-prime credits the compiled code: to the one file
/// whose module defined a symbol of that name, internal or not. A name
/// that two did (static functions alike), or none (literals, or a
/// static LTO renamed to keep it apart), stays the compiled object's.
pub fn origins(inputs: &[LtoInput]) -> hashbrown::HashMap<&'static str, Option<usize>> {
    let mut map = hashbrown::HashMap::new();
    for input in inputs {
        for &name in &input.defined {
            map.entry(name)
                .and_modify(|origin: &mut Option<usize>| {
                    if *origin != Some(input.obj) {
                        *origin = None;
                    }
                })
                .or_insert(Some(input.obj));
        }
    }
    map
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
    let triple = module_triple(plugin, module as usize);
    // SAFETY: the module handle is valid and no longer used.
    unsafe { (plugin.module_dispose)(module) };
    triple
}

/// The target triple of a module parse_module created.
pub fn module_triple(plugin: &Plugin, module: usize) -> String {
    // SAFETY: the module handle is valid, and the triple is a
    // NUL-terminated string it owns.
    unsafe {
        let triple = CStr::from_ptr((plugin.module_get_target_triple)(module as *mut c_void));
        triple.to_string_lossy().into_owned()
    }
}

/// Whether clang built a module parse_module created for ThinLTO.
pub fn module_is_thin(plugin: &Plugin, module: usize) -> bool {
    // SAFETY: the module handle is valid.
    unsafe { (plugin.module_is_thinlto)(module as *mut c_void) }
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
