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
use std::path::{Path, PathBuf};

use mold_common::bytes::display;
use mold_common::path::path_bytes;
use mold_common::{fatal, warn};
use rayon::prelude::*;

use crate::arch::Target;
use crate::context::Context;
use crate::dead_strip::keeps_export;
use crate::filetype::without_fat_arch;
use crate::input_files::{FileId, ObjectFile, PlatformVersion, ignore_foreign_file};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::symbol::{Symbol, SymbolId};

// Symbol attribute bits from llvm-c/lto.h
const LTO_SYMBOL_DEFINITION_MASK: u32 = 0x700;
const LTO_SYMBOL_DEFINITION_REGULAR: u32 = 0x100;
const LTO_SYMBOL_DEFINITION_TENTATIVE: u32 = 0x200;
const LTO_SYMBOL_DEFINITION_WEAK: u32 = 0x300;
const LTO_SYMBOL_SCOPE_MASK: u32 = 0x3800;
const LTO_SYMBOL_SCOPE_INTERNAL: u32 = 0x800;
const LTO_SYMBOL_SCOPE_HIDDEN: u32 = 0x1000;
const LTO_SYMBOL_SCOPE_DEFAULT_CAN_BE_HIDDEN: u32 = 0x2800;

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
        unsafe { c_string(self.get_error_message).unwrap_or_else(|| "unknown error".into()) }
    }
}

/// The string a libLTO function returns, if any: bytes, which may name
/// a file.
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
#[cfg(not(windows))]
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
#[cfg(not(windows))]
const DEFAULT_LTO_LIBRARY: &str =
    if cfg!(target_os = "macos") { "libLTO.dylib" } else { "libLTO.so" };

/// The LTO library of the toolchain the linker is installed in, if
/// there is one: ld-prime links the libLTO in lib beside its bin
/// directory (@rpath, which is @executable_path/../lib), as ld64 looked
/// for it by its own real path, and clang passes -lto_library with the
/// one beside it in the same way.
#[cfg(not(windows))]
fn toolchain_lto_library() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let path = exe.parent()?.parent()?.join("lib").join(DEFAULT_LTO_LIBRARY);
    path.is_file().then_some(path)
}

/// Loads libLTO from the given path (from -lto_library), or else the
/// linker's toolchain's, or else the one the dynamic loader finds by
/// name in its search path.
#[cfg(not(windows))]
pub fn load_plugin(path: Option<&Path>) -> Plugin {
    let default = path.is_none().then(toolchain_lto_library).flatten();
    let path = path.or(default.as_deref());
    let path =
        CString::new(path.map_or(DEFAULT_LTO_LIBRARY.as_bytes(), mold_common::path::path_bytes))
            .unwrap_or_else(|_| fatal!("-lto_library: path contains a NUL byte"));
    // SAFETY: dlopen/dlsym with valid NUL-terminated strings.
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            fatal!(
                "could not load the LTO library {}; is -lto_library missing?",
                display(path.as_bytes())
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

#[cfg(windows)]
pub fn load_plugin(_path: Option<&Path>) -> Plugin {
    fatal!("LTO is not supported on Windows");
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
                && let Ok(path) = CString::new(path.as_encoded_bytes())
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
    let path = CString::new(mold_common::path::path_bytes(path))
        .map_err(|_| String::from("output path contains a NUL byte"))?;
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

/// Creates a directory, as ld-prime does ThinLTO's, owner-only.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(windows)]
    {
        std::fs::create_dir(path)
    }
}

/// Hands ThinLTO the cache directory, which ld-prime creates (one level
/// of it, owner-only) if it is not one yet - or warns and goes without.
///
/// # Safety
///
/// `cg` must be a live ThinLTO code generator.
unsafe fn set_cache(plugin: &Plugin, cg: *mut c_void, cache: &CacheOptions) {
    if !cache.dir.is_dir()
        && let Err(e) = create_private_dir(cache.dir)
    {
        let errno = e.raw_os_error().unwrap_or(0);
        warn!("unable to create ThinLTO cache directory: {} ({errno})", cache.dir.display());
        return;
    }
    let dir = CString::new(mold_common::path::path_bytes(cache.dir)).unwrap_or_default();
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
    preserve: &[&[u8]],
    cross: &[&[u8]],
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
            (plugin.thinlto_codegen_set_savetemps_dir)(cg, c(dir.as_encoded_bytes()).as_ptr());
        }
        let objects_dir = opts.objects_dir.map(|dir| c(mold_common::path::path_bytes(dir)));
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
                 object): '{}'",
                plugin.error_message()
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
    let dir = temp_path(output, ".thinlto.bcs/");
    if !Path::new(&dir).is_dir() && create_private_dir(Path::new(&dir)).is_err() {
        warn!(
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
            warn!("unable to write temporary ThinLTO output: {}", path.display());
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
                warn!("Ignoring empty buffer generated by ThinLTO");
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
            let path = std::path::PathBuf::from(mold_common::bytes::os_str(path.to_bytes()));
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
    pub defined: Vec<&'static [u8]>,
    /// Whether clang built the module for ThinLTO (-flto=thin).
    pub is_thin: bool,
}

/// A bitcode file LTO compiled, as the passes after LTO still see it
/// once its placeholder object is retired.
pub struct LtoInput {
    /// The placeholder object.
    pub obj: usize,
    /// The names the module defined, internal ones included.
    pub defined: Vec<&'static [u8]>,
    /// The external symbols resolution gave the file's definitions.
    pub won: Vec<&'static [u8]>,
}

/// The bitcode file each symbol of the objects LTO compiled comes from,
/// by name, as ld-prime credits the compiled code: an external symbol
/// to the file whose definition won it (the first copy of a weak one),
/// and another name to the one file whose module defined it. A name
/// that two did (static functions alike), or none (literals, or a
/// static LTO renamed to keep it apart), stays the compiled object's.
pub fn origins(inputs: &[LtoInput]) -> hashbrown::HashMap<&'static [u8], Option<usize>> {
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
    for input in inputs {
        for &name in &input.won {
            map.insert(name, Some(input.obj));
        }
    }
    map
}

/// A parsed bitcode module's symbol, in linker terms. libLTO gives its
/// name as a C string: bytes, UTF-8 or not.
pub struct LtoSymbol {
    pub name: &'static [u8],
    pub is_defined: bool,
    pub is_weak_def: bool,
    pub is_extern: bool,
    pub is_private_extern: bool,
    /// A weak definition no one can tell the copy of (linkonce_odr
    /// with unnamed_addr), as .weak_def_can_be_hidden marks one.
    pub can_be_hidden: bool,
}

/// Creates a module from a bitcode buffer.
fn create_module(plugin: &Plugin, data: &[u8], name: &Path) -> *mut c_void {
    let cname = CString::new(mold_common::path::path_bytes(name)).unwrap_or_default();
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

/// Frees a module parse_module created that the link doesn't take.
pub fn dispose_module(plugin: &Plugin, module: usize) {
    // SAFETY: the module handle is valid and no longer used.
    unsafe { (plugin.module_dispose)(module as *mut c_void) };
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
                name: mold_common::mem::leak_bytes(cstr.to_bytes().to_vec()),
                is_defined: matches!(
                    def,
                    LTO_SYMBOL_DEFINITION_REGULAR
                        | LTO_SYMBOL_DEFINITION_TENTATIVE
                        | LTO_SYMBOL_DEFINITION_WEAK
                ),
                is_weak_def: def == LTO_SYMBOL_DEFINITION_WEAK,
                is_extern: scope != LTO_SYMBOL_SCOPE_INTERNAL && scope != 0,
                is_private_extern: scope == LTO_SYMBOL_SCOPE_HIDDEN,
                can_be_hidden: scope == LTO_SYMBOL_SCOPE_DEFAULT_CAN_BE_HIDDEN,
            });
        }
    }
    (module as usize, syms)
}

/// Loads the LTO plugin on first use.
fn ensure_lto_plugin<E: Target>(ctx: &mut Context<E>) -> Plugin {
    if ctx.lto_plugin.is_none() {
        ctx.lto_plugin = Some(load_plugin(ctx.args.lto_library.as_deref()));
    }
    ctx.lto_plugin.unwrap()
}

/// Registers a bitcode input: a placeholder object that claims the
/// module's symbols so resolution works, compiled for real by LTO once
/// all inputs are known. One for another architecture than the link's
/// is ignored, as ld-prime ignores a Mach-O object (see
/// reader::is_foreign): None.
pub fn read_lto_object<E: Target>(
    ctx: &mut Context<E>,
    mf: &'static MappedFile,
    alive: bool,
) -> Option<usize> {
    let plugin = ensure_lto_plugin(ctx);
    let (module, lsyms) = parse_module(&plugin, mf.data(), &mf.name);
    if let Some(arch) = foreign_bitcode_arch::<E>(&plugin, module) {
        if ctx.args.allow_sub_type_mismatches && is_bitcode_subtype_mismatch::<E>(&arch) {
            let name = without_fat_arch(mold_common::path::path_bytes(&mf.name));
            warn!("linking {arch} file '{}' into {} link", display(&name), E::NAME);
        } else {
            let why = format!("found architecture '{arch}', required architecture '{}'", E::NAME);
            ignore_foreign_file(ctx, mf, &why);
            dispose_module(&plugin, module);
            return None;
        }
    }
    // ld-prime checks the target triple's OS and version as it checks
    // a Mach-O object's platform load command.
    let triple = module_triple(&plugin, module);
    let platform_versions = PlatformVersion::of_triple(&triple).into_iter().collect();

    // The module's symbols become MachSyms, so that resolution handles
    // bitcode like any object; its internal definitions are left out.
    let mut defined = Vec::new();
    let mut mach_syms = Vec::new();
    let mut syms = Vec::new();
    for ls in lsyms {
        if ls.is_defined {
            defined.push(ls.name);
        }
        if ls.is_extern || !ls.is_defined {
            syms.push(ctx.symbols.intern(ls.name));
            mach_syms.push(bitcode_msym(&ls));
        }
    }

    let obj_idx = ctx.objs.len();
    let priority = ctx.next_priority();
    ctx.objs.push(ObjectFile {
        is_reachable: alive,
        priority,
        platform_versions,
        sym_subsecs: vec![crate::symbol::NONE; mach_syms.len()],
        mach_syms: std::borrow::Cow::Owned(mach_syms),
        symbols: syms,
        lto_module: Some(module),
        ..ObjectFile::new(mf)
    });
    let is_thin = module_is_thin(&plugin, module);
    ctx.lto_modules.push(BitcodeModule { obj: obj_idx, handle: module, defined, is_thin });
    Some(obj_idx)
}

/// The MachSym an external symbol of a bitcode module stands for: an
/// absolute definition, or an undefined reference.
fn bitcode_msym(ls: &LtoSymbol) -> MachSym {
    let mut msym = MachSym::default();
    if !ls.is_defined {
        msym.n_type = N_UNDF | N_EXT;
        return msym;
    }
    msym.n_type = N_ABS | N_EXT | if ls.is_private_extern { N_PEXT } else { 0 };
    if ls.is_weak_def {
        msym.desc.set(msym.desc.get() | N_WEAK_DEF);
    }
    if ls.is_weak_def && ls.can_be_hidden {
        msym.desc.set(msym.desc.get() | N_WEAK_REF);
    }
    msym
}

/// The architecture a bitcode module was compiled for, from its target
/// triple (x86_64h-apple-macosx14.0.0), if the link doesn't take it -
/// named as for a Mach-O file, a Thumb one (thumbv7-apple-ios9.0.0) by
/// its ARM architecture.
fn foreign_bitcode_arch<E: Target>(plugin: &Plugin, module: usize) -> Option<String> {
    let triple = module_triple(plugin, module);
    let arch = match triple.split('-').next().unwrap_or_default() {
        "aarch64" => "arm64".to_string(),
        arch => match arch.strip_prefix("thumb") {
            Some(version) => format!("arm{version}"),
            None => arch.to_string(),
        },
    };
    (arch != E::NAME).then_some(arch)
}

/// Whether a bitcode module of architecture `arch` is of the link's CPU
/// type all the same (see filetype::is_subtype_mismatch).
fn is_bitcode_subtype_mismatch<E: Target>(arch: &str) -> bool {
    match E::NAME {
        "x86_64" => arch == "x86_64h",
        _ => false,
    }
}

/// The bitcode modules of live files, in input order.
pub fn live_bitcode_modules<E: Target>(ctx: &Context<E>) -> impl Iterator<Item = &BitcodeModule> {
    ctx.lto_modules.iter().filter(|module| ctx.objs[module.obj].is_reachable)
}

/// Writes a -r link of bitcode alone as one merged bitcode file (see
/// passes::links_only_bitcode). ld-prime warns, then fails, if libLTO
/// can't.
pub fn write_merged_bitcode<E: Target>(ctx: &Context<E>) {
    let plugin = ctx.lto_plugin.unwrap();
    let modules: Vec<_> = live_bitcode_modules(ctx).collect();
    let roots = lto_roots(ctx);
    // SAFETY: libLTO calls with handles created by the same library.
    unsafe {
        let cg = create_lto_codegen(ctx, &plugin, &modules, &roots);
        if let Err(msg) = write_merged_modules(&plugin, cg, &ctx.args.output) {
            warn!("could not produce merged bitcode file");
            fatal!("LTO codegen error: {msg}");
        }
    }
}

/// Creates libLTO's code generator for the modules to merge, added in
/// input order, with the symbols that must survive LTO.
///
/// # Safety
///
/// The plugin must be the library the modules were created by.
unsafe fn create_lto_codegen<E: Target>(
    ctx: &Context<E>,
    plugin: &Plugin,
    modules: &[&BitcodeModule],
    roots: &[&[u8]],
) -> *mut std::ffi::c_void {
    // SAFETY: libLTO calls with handles created by the same library.
    unsafe {
        let cg = (plugin.codegen_create)();
        if cg.is_null() {
            fatal!("lto_codegen_create failed: {}", plugin.error_message());
        }
        (plugin.codegen_set_pic_model)(cg, LTO_CODEGEN_PIC_MODEL_DYNAMIC);
        set_debug_options(plugin, cg, &ctx.args.mllvm);
        for module in modules {
            if (plugin.codegen_add_module)(cg, module.handle as *mut _) {
                fatal!("lto_codegen_add_module failed: {}", plugin.error_message());
            }
        }
        for name in roots {
            if let Ok(name) = std::ffi::CString::new(*name) {
                (plugin.codegen_add_must_preserve_symbol)(cg, name.as_ptr());
            }
        }
        cg
    }
}

/// The symbols of the bitcode modules that must survive the LTO
/// internalizer, as ld-prime picks them - the same set for ThinLTO and
/// the merged module: the definitions the output exports (see
/// exported_before_lto), the entry point, -u symbols, -alias bases
/// (which the linker itself references), and those some code outside
/// the module defining them references: live Mach-O code (see
/// dead_strip::native_refs_before_lto), or a bitcode module that
/// libLTO compiles apart from it. A reference between two modules it
/// merges does not count, as libLTO resolves it itself (_times2,
/// called only from a bitcode main, goes local and is not exported),
/// but a ThinLTO module is compiled on its own, so a reference to or
/// from one does. A native common counts: when a bitcode definition
/// wins, the common's code addresses that definition's storage.
fn lto_roots<E: Target>(ctx: &Context<E>) -> Vec<&[u8]> {
    use std::sync::atomic::{AtomicU8, Ordering};

    // Who refers to each symbol: a ThinLTO module or a module to merge
    // (one whose copy of a weak definition another's replaced counts,
    // as mold's LTO plugin calls such a copy preempted: its code has to
    // reach the copy that won, or a C++ inline function's static local
    // would split in two); whether a Mach-O object defines it; and
    // whether it has a weak definition and one that can't be hidden.
    const THIN_REF: u8 = 1;
    const MERGED_REF: u8 = 2;
    const NATIVE_DEF: u8 = 4;
    const WEAK: u8 = 8;
    const NOT_HIDABLE: u8 = 16;
    let mut thin = vec![None; ctx.objs.len()];
    for module in &ctx.lto_modules {
        thin[module.obj] = Some(module.is_thin);
    }
    let flags: Vec<AtomicU8> = (0..ctx.symbols.syms.len()).map(|_| AtomicU8::new(0)).collect();
    ctx.objs.par_iter().enumerate().filter(|(_, obj)| obj.is_reachable).for_each(|(i, obj)| {
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if msym.is_stab() || !msym.is_extern() {
                continue;
            }
            let defined = matches!(msym.ty(), N_SECT | N_ABS);
            let lost = || ctx.symbols[sym_id].file() != Some(FileId::Obj(i as u32));
            let mut flag = match (msym.ty(), thin[i]) {
                (N_UNDF, Some(true)) => THIN_REF,
                (N_UNDF, Some(false)) => MERGED_REF,
                (N_SECT | N_ABS, None) => NATIVE_DEF,
                (N_ABS, Some(true)) if lost() => THIN_REF,
                (N_ABS, Some(false)) if lost() => MERGED_REF,
                _ => 0,
            };
            if defined && msym.desc.get() & N_WEAK_DEF != 0 {
                flag |= if msym.desc.get() & N_WEAK_REF != 0 { WEAK } else { WEAK | NOT_HIDABLE };
            } else if defined {
                flag |= NOT_HIDABLE;
            }
            flags[sym_id as usize].fetch_or(flag, Ordering::Relaxed);
        }
    });
    let exported = |id: SymbolId| {
        let f = flags[id as usize].load(Ordering::Relaxed);
        let hidable = f & (WEAK | NOT_HIDABLE) == WEAK;
        exported_before_lto(ctx, &ctx.symbols[id], hidable)
    };
    let native_refs = crate::dead_strip::native_refs_before_lto(ctx, exported);

    let mut roots = Vec::new();
    for (i, sym) in ctx.symbols.syms.iter().enumerate() {
        let Some(FileId::Obj(obj)) = sym.file() else { continue };
        let Some(is_thin) = thin[obj as usize] else { continue };
        if !ctx.objs[obj as usize].is_reachable || !sym.is_extern() {
            continue;
        }
        let outside = THIN_REF | if is_thin { MERGED_REF } else { 0 };
        let name = sym.name();
        if native_refs[i].load(Ordering::Relaxed)
            || flags[i].load(Ordering::Relaxed) & outside != 0
            || exported(i as SymbolId)
            || ctx.args.command_line_symbols().any(|named| named == name)
        {
            roots.push(name);
        }
    }

    // A bitcode definition a native object has one of too survives, as
    // ld64 keeps the LLVM definitions it coalesced away in favor of
    // Mach-O ones: left to libLTO, a weak one would be inlined into the
    // module's callers in place of the strong native definition that
    // wins, and a strong one would vanish rather than be reported as a
    // duplicate (ld-prime lists it in the compiled object).
    for module in live_bitcode_modules(ctx) {
        let obj = &ctx.objs[module.obj];
        for (msym, &id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if msym.ty() == N_ABS && flags[id as usize].load(Ordering::Relaxed) & NATIVE_DEF != 0 {
                roots.push(ctx.symbols[id].name());
            }
        }
    }
    roots
}

/// Whether a definition is exported before LTO, as ld-prime's walk
/// before it and libLTO's preserve set see it: under an export list if
/// the list names it, hidden or not; otherwise, unless
/// -unexported_symbols_list names it, any external definition in -r,
/// and a visible one in an image that exports any (see
/// dead_strip::keeps_export) - but not one every copy of which can be
/// hidden, which the image auto-hides (see
/// passes::auto_hide_weak_defs).
fn exported_before_lto<E: Target>(ctx: &Context<E>, sym: &Symbol, hidable: bool) -> bool {
    if !sym.is_extern() || !matches!(sym.file(), Some(FileId::Obj(_))) {
        return false;
    }
    let name = sym.name();
    if let Some(exported) = &ctx.args.exported_symbols {
        return exported.find(name) != -1;
    }
    if ctx.args.unexported_symbols.find(name) != -1 {
        return false;
    }
    ctx.args.relocatable || (!sym.is_private_extern() && !hidable && keeps_export(ctx, sym.name()))
}

/// An object LTO compiled, under the name ld-prime gives it (see
/// thin_lto and merged_lto) and the modification time its debug stab
/// gets if not the named file's.
pub struct LtoObject {
    pub name: PathBuf,
    pub mtime: Option<u64>,
    pub data: Vec<u8>,
}

/// Compiles the live bitcode modules to Mach-O objects as ld-prime
/// does - first the modules built for ThinLTO, an object each, then the
/// rest merged into one - for passes::do_lto to put in place of the
/// bitcode files. Both compilations see the same symbols to preserve.
/// -flto-codegen-only has ThinLTO compile every module, unoptimized.
pub fn run_plugin<E: Target>(ctx: &Context<E>) -> Vec<LtoObject> {
    let plugin = ctx.lto_plugin.unwrap();

    let mut objects = Vec::new();
    let roots = lto_roots(ctx);
    let (thin, merged): (Vec<_>, Vec<_>) =
        live_bitcode_modules(ctx).partition(|module| module.is_thin || ctx.args.lto_codegen_only);
    if !thin.is_empty() {
        objects.extend(thin_lto(ctx, &plugin, &thin, &roots));
    }
    if !merged.is_empty() {
        objects.push(merged_lto(ctx, &plugin, &merged, &roots));
    }
    objects
}

/// Compiles the ThinLTO modules to an object each. libLTO tells the
/// modules apart by name: ld-prime gives each its file's real path -
/// an archive member's as archive[index](member) - followed by its
/// index among them. It names the objects after the files libLTO wrote
/// to the -object_path_lto directory, or else not at all: an empty
/// name in diagnostics, the map and the debug stab (whose modification
/// time is then 0).
fn thin_lto<E: Target>(
    ctx: &Context<E>,
    plugin: &Plugin,
    modules: &[&BitcodeModule],
    roots: &[&[u8]],
) -> Vec<LtoObject> {
    let thin_modules: Vec<ThinModule> = modules
        .iter()
        .enumerate()
        .map(|(i, module)| {
            let mf = ctx.objs[module.obj].mf;
            let mut id = path_bytes(&mf.name).to_vec();
            id.extend_from_slice(i.to_string().as_bytes());
            let id = std::ffi::CString::new(id).unwrap_or_default();
            ThinModule { id, data: mf.data() }
        })
        .collect();

    // What the modules refer to, defined anywhere, ThinLTO keeps too.
    let mut cross = Vec::new();
    for module in modules {
        let obj = &ctx.objs[module.obj];
        for (msym, &id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if msym.ty() == N_UNDF {
                cross.push(ctx.symbols[id].name());
            }
        }
    }

    let opts = ThinOptions {
        debug_options: &ctx.args.mllvm,
        cpu: ctx.args.lto_cpu.as_deref(),
        objects_dir: ctx.args.object_path_lto.as_deref(),
        cache: ctx.args.lto_cache_dir.as_deref().map(|dir| CacheOptions {
            dir,
            prune_interval: ctx.args.lto_cache_prune_interval,
            expiration: ctx.args.lto_cache_expiration,
            max_size: ctx.args.lto_cache_max_size,
        }),
        save_temps: ctx.args.save_temps.then_some(ctx.args.output.as_path()),
        codegen_only: ctx.args.lto_codegen_only,
    };
    // SAFETY: the plugin is the library that parsed the modules.
    let objects = unsafe { compile_thin(plugin, &thin_modules, roots, &cross, &opts) };
    objects
        .into_iter()
        .map(|obj| match obj.path {
            Some(name) => LtoObject { name, mtime: None, data: obj.data },
            None => LtoObject { name: PathBuf::new(), mtime: Some(0), data: obj.data },
        })
        .collect()
}

/// Merges the other modules into one and compiles it to one object.
/// -object_path_lto keeps that object: debug info stays in object files
/// on Mach-O (the executable only gets stabs pointing at them), and for
/// LTO code the object exists only inside the linker - Xcode passes a
/// path under the dSYM staging directory so dsymutil can find it
/// afterwards. ld-prime names the object after that file, or else after
/// a temporary file it never writes, in its diagnostics, the map and
/// the debug stabs - which give the latter modification time 0.
fn merged_lto<E: Target>(
    ctx: &Context<E>,
    plugin: &Plugin,
    modules: &[&BitcodeModule],
    roots: &[&[u8]],
) -> LtoObject {
    // SAFETY: libLTO calls with handles created by the same library.
    let data = unsafe {
        let cg = create_lto_codegen(ctx, plugin, modules, roots);
        let opts = CodegenOptions {
            cpu: ctx.args.lto_cpu.as_deref(),
            save_temps: ctx.args.save_temps.then_some(ctx.args.output.as_path()),
        };
        compile(plugin, cg, &opts)
    };
    match &ctx.args.object_path_lto {
        Some(path) => {
            let path = lto_object_path(path);
            // ld-prime keeps the object if it can, saying nothing
            // otherwise.
            let _ = std::fs::write(&path, &data);
            LtoObject { name: path, mtime: None, data }
        }
        None => LtoObject { name: PathBuf::from("/tmp/lto.o"), mtime: Some(0), data },
    }
}

/// Where -object_path_lto has the merged LTO object written: the path
/// itself, or lto.o in it if it names a directory - as it does when
/// ThinLTO objects share it (ld-prime appends "/lto.o" to the path as
/// given, trailing slash or not).
fn lto_object_path(path: &Path) -> PathBuf {
    if !path.is_dir() {
        return path.to_path_buf();
    }
    let mut path = path.as_os_str().to_owned();
    path.push("/lto.o");
    PathBuf::from(path)
}
