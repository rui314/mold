//! Link-time optimization via libLTO.
//!
//! With -flto, clang emits object files that are LLVM bitcode rather
//! than Mach-O. The linker is expected to load Apple's libLTO.dylib
//! (clang passes its path as -lto_library), register every bitcode
//! module, tell the library which symbols must survive, and compile
//! them all into one Mach-O object that then joins the link like any
//! other input.

use std::ffi::{CStr, CString, c_char, c_void};

use crate::fatal;

// Symbol attribute bits from llvm-c/lto.h
pub const LTO_SYMBOL_DEFINITION_MASK: u32 = 0x700;
pub const LTO_SYMBOL_DEFINITION_REGULAR: u32 = 0x100;
pub const LTO_SYMBOL_DEFINITION_TENTATIVE: u32 = 0x200;
pub const LTO_SYMBOL_DEFINITION_WEAK: u32 = 0x300;
pub const LTO_SYMBOL_DEFINITION_UNDEFINED: u32 = 0x400;
pub const LTO_SYMBOL_DEFINITION_WEAKUNDEF: u32 = 0x500;
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
    pub codegen_create: unsafe extern "C" fn() -> *mut c_void,
    pub codegen_add_module: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    pub codegen_set_pic_model: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    pub codegen_add_must_preserve_symbol: unsafe extern "C" fn(*mut c_void, *const c_char),
    pub codegen_compile: unsafe extern "C" fn(*mut c_void, *mut usize) -> *const c_void,
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

/// Loads libLTO from the given path (from -lto_library, with a plain
/// "libLTO.dylib" fallback that relies on dyld's search).
#[cfg(not(windows))]
pub fn load_plugin(path: Option<&str>) -> Plugin {
    let path = CString::new(path.unwrap_or("libLTO.dylib")).unwrap();
    // SAFETY: dlopen/dlsym with valid NUL-terminated strings.
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            fatal!(
                "could not load the LTO library {}; is -lto_library missing?",
                path.to_string_lossy()
            );
        }

        macro_rules! dlsym {
            ($name:literal) => {{
                let sym = libc::dlsym(handle, concat!($name, "\0").as_ptr() as *const c_char);
                if sym.is_null() {
                    fatal!("libLTO does not provide {}", $name);
                }
                // The function pointer type is the field's, which differs per
                // expansion, so it cannot be spelled here.
                #[allow(clippy::missing_transmute_annotations)]
                std::mem::transmute(sym)
            }};
        }

        Plugin {
            get_error_message: dlsym!("lto_get_error_message"),
            module_create_from_memory_with_path: dlsym!("lto_module_create_from_memory_with_path"),
            module_dispose: dlsym!("lto_module_dispose"),
            module_get_num_symbols: dlsym!("lto_module_get_num_symbols"),
            module_get_symbol_name: dlsym!("lto_module_get_symbol_name"),
            module_get_symbol_attribute: dlsym!("lto_module_get_symbol_attribute"),
            codegen_create: dlsym!("lto_codegen_create"),
            codegen_add_module: dlsym!("lto_codegen_add_module"),
            codegen_set_pic_model: dlsym!("lto_codegen_set_pic_model"),
            codegen_add_must_preserve_symbol: dlsym!("lto_codegen_add_must_preserve_symbol"),
            codegen_compile: dlsym!("lto_codegen_compile"),
        }
    }
}

#[cfg(windows)]
pub fn load_plugin(_path: Option<&str>) -> Plugin {
    fatal!("LTO is not supported on Windows");
}

/// A parsed bitcode module's symbol, in linker terms.
pub struct LtoSymbol {
    pub name: String,
    pub is_defined: bool,
    pub is_weak_def: bool,
    pub is_extern: bool,
    pub is_private_extern: bool,
}

/// Creates a module from a bitcode buffer and lists its symbols.
pub fn parse_module(plugin: &Plugin, data: &[u8], name: &str) -> (usize, Vec<LtoSymbol>) {
    let cname = CString::new(name).unwrap_or_default();
    // SAFETY: the buffer is valid for the call's duration; libLTO copies
    // what it needs.
    let module = unsafe {
        (plugin.module_create_from_memory_with_path)(
            data.as_ptr() as *const c_void,
            data.len(),
            cname.as_ptr(),
        )
    };
    if module.is_null() {
        fatal!("{name}: lto_module_create failed: {}", plugin.error_message());
    }

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
