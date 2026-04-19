use std::env;

fn defined(var: &str) -> bool {
    println!("cargo:rerun-if-env-changed={}", var);
    env::var_os(var).is_some()
}

fn is_pure() -> bool {
    defined("CARGO_FEATURE_PURE")
}

fn should_prefer_intrinsics() -> bool {
    defined("CARGO_FEATURE_PREFER_INTRINSICS")
}

fn is_neon() -> bool {
    defined("CARGO_FEATURE_NEON")
}

fn is_no_neon() -> bool {
    defined("CARGO_FEATURE_NO_NEON")
}

fn is_wasm32_simd() -> bool {
    defined("CARGO_FEATURE_WASM32_SIMD")
}

fn is_ci() -> bool {
    defined("BLAKE3_CI")
}

fn warn(warning: &str) {
    assert!(!warning.contains("\n"));
    println!("cargo:warning={}", warning);
    if is_ci() {
        println!("cargo:warning=Warnings in CI are treated as errors. Build failed.");
        std::process::exit(1);
    }
}

fn target_components() -> Vec<String> {
    let target = env::var("TARGET").unwrap();
    target.split("-").map(|s| s.to_string()).collect()
}

fn is_x86_64() -> bool {
    target_components()[0] == "x86_64"
}

fn is_windows_target() -> bool {
    env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows"
}

fn use_msvc_asm() -> bool {
    const MSVC_NAMES: &[&str] = &["", "cl", "cl.exe"];
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_windows_msvc = target_os == "windows" && target_env == "msvc";
    let host_triple = env::var("HOST").unwrap_or_default();
    let target_triple = env::var("TARGET").unwrap_or_default();
    let cross_compiling = host_triple != target_triple;
    let cc = env::var("CC").unwrap_or_default().to_ascii_lowercase();
    if !target_windows_msvc {
        // We are not building for Windows with the MSVC toolchain.
        false
    } else if !cross_compiling && MSVC_NAMES.contains(&&*cc) {
        // We are building on Windows with the MSVC toolchain (and not cross-compiling for another architecture or target).
        true
    } else {
        // We are cross-compiling to Windows with the MSVC toolchain.
        let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let target_vendor = env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
        let cc = env::var(format!("CC_{target_arch}_{target_vendor}_windows_msvc"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        // Check if we are using the MSVC compiler.
        MSVC_NAMES.contains(&&*cc)
    }
}

fn is_x86_32() -> bool {
    let arch = &target_components()[0];
    arch == "i386" || arch == "i586" || arch == "i686"
}

fn is_arm() -> bool {
    is_armv7() || is_aarch64() || target_components()[0] == "arm"
}

fn is_aarch64() -> bool {
    target_components()[0] == "aarch64"
}

fn is_armv7() -> bool {
    target_components()[0] == "armv7"
}

fn is_wasm32() -> bool {
    target_components()[0] == "wasm32"
}

fn endianness() -> String {
    let endianness = env::var("CARGO_CFG_TARGET_ENDIAN").unwrap();
    assert!(endianness == "little" || endianness == "big");
    endianness
}

fn is_little_endian() -> bool {
    endianness() == "little"
}

fn is_big_endian() -> bool {
    endianness() == "big"
}

// Windows targets may be using the MSVC toolchain or the MinGW toolchain. The
// right compiler flags to use depend on the toolchain. (And we don't want to
// use flag_if_supported, because we don't want features to be silently
// disabled by old compilers.)
fn is_windows_msvc() -> bool {
    // Some targets are only two components long, so check in steps.
    let second_component = &target_components()[1];
    (second_component == "pc" || second_component == "win7")
        && target_components()[2] == "windows"
        && target_components()[3] == "msvc"
}

// MinGW toolchain uses 2 different targets depending on the main compiler.
// Target for a general MinGW toolchain ends with `-gnu` (GCC is used as C
// compiler). Target for a LLVM-MinGW toolchain (Clang is used as C compiler)
// ends with `-gnullvm`.
fn is_windows_gnu() -> bool {
    // Some targets are only two components long, so check in steps.
    let second_component = &target_components()[1];
    (second_component == "pc" || second_component == "win7")
        && target_components()[2] == "windows"
        && target_components()[3] != "msvc"
}

fn new_build() -> cc::Build {
    let mut build = cc::Build::new();
    if !is_windows_msvc() {
        build.flag("-std=c11");
    }
    // Do NOT trigger a rebuild any time the env changes (e.g. $PATH).
    // This prevents all downstream crates from being rebuilt when `cargo check`
    // or `cargo build` are run in different environments, like Rust Analyzer
    // vs. in the terminal vs. in a Git pre-commit hook.
    build.emit_rerun_if_env_changed(false);
    build
}

#[derive(PartialEq)]
enum CCompilerSupport {
    NoCompiler,
    NoAVX512,
    YesAVX512,
}
use CCompilerSupport::*;

fn c_compiler_support() -> CCompilerSupport {
    let build = new_build();
    let flags_checked;
    let support_result: Result<bool, _> = if is_windows_msvc() {
        flags_checked = "/arch:AVX512";
        build.is_flag_supported("/arch:AVX512")
    } else {
        // Check for both of the flags we use. If -mavx512f works, then -mavx512vl
        // will probably always work too, but we might as well be thorough.
        flags_checked = "-mavx512f and -mavx512vl";
        match build.is_flag_supported("-mavx512f") {
            Ok(true) => build.is_flag_supported("-mavx512vl"),
            false_or_error => false_or_error,
        }
    };
    match support_result {
        Ok(true) => YesAVX512,
        Ok(false) => {
            warn(&format!(
                "The C compiler {:?} does not support {}.",
                build.get_compiler().path(),
                flags_checked,
            ));
            NoAVX512
        }
        Err(e) => {
            println!("{:?}", e);
            warn(&format!(
                "No C compiler {:?} detected.",
                build.get_compiler().path()
            ));
            NoCompiler
        }
    }
}

fn build_sse2_sse41_avx2_rust_intrinsics() {
    // No C code to compile here. Set the cfg flags that enable the Rust SSE2,
    // SSE4.1, and AVX2 intrinsics modules. The regular Cargo build will compile
    // them.
    println!("cargo:rustc-cfg=blake3_sse2_rust");
    println!("cargo:rustc-cfg=blake3_sse41_rust");
    println!("cargo:rustc-cfg=blake3_avx2_rust");
}

fn build_sse2_sse41_avx2_assembly() {
    // Build the assembly implementations for SSE4.1 and AVX2. This is
    // preferred, but it only supports x86_64.
    assert!(is_x86_64());
    println!("cargo:rustc-cfg=blake3_sse2_ffi");
    println!("cargo:rustc-cfg=blake3_sse41_ffi");
    println!("cargo:rustc-cfg=blake3_avx2_ffi");
    let mut build = new_build();
    if is_windows_target() {
        if use_msvc_asm() {
            build.file("c/blake3_sse2_x86-64_windows_msvc.asm");
            build.file("c/blake3_sse41_x86-64_windows_msvc.asm");
            build.file("c/blake3_avx2_x86-64_windows_msvc.asm");
        } else {
            build.file("c/blake3_sse2_x86-64_windows_gnu.S");
            build.file("c/blake3_sse41_x86-64_windows_gnu.S");
            build.file("c/blake3_avx2_x86-64_windows_gnu.S");
        }
    } else {
        // All non-Windows implementations are assumed to support
        // Linux-style assembly. These files do contain a small
        // explicit workaround for macOS also.
        build.file("c/blake3_sse2_x86-64_unix.S");
        build.file("c/blake3_sse41_x86-64_unix.S");
        build.file("c/blake3_avx2_x86-64_unix.S");
    }
    build.compile("blake3_sse2_sse41_avx2_assembly");
}

fn build_avx512_c_intrinsics() {
    // This is required on 32-bit x86 targets, since the assembly
    // implementation doesn't support those.
    println!("cargo:rustc-cfg=blake3_avx512_ffi");
    let mut build = new_build();
    build.file("c/blake3_avx512.c");
    if is_windows_msvc() {
        build.flag("/arch:AVX512");
    } else {
        build.flag("-mavx512f");
        build.flag("-mavx512vl");
    }
    if is_windows_gnu() {
        // Workaround for https://gcc.gnu.org/bugzilla/show_bug.cgi?id=65782.
        build.flag("-fno-asynchronous-unwind-tables");
    }
    build.compile("blake3_avx512_intrinsics");
}

fn build_avx512_assembly() {
    // Build the assembly implementation for AVX-512. This is preferred, but it
    // only supports x86_64.
    assert!(is_x86_64());
    println!("cargo:rustc-cfg=blake3_avx512_ffi");
    let mut build = new_build();
    let mut is_msvc = false;
    if is_windows_target() {
        if use_msvc_asm() {
            build.file("c/blake3_avx512_x86-64_windows_msvc.asm");
            is_msvc = true;
        } else {
            build.file("c/blake3_avx512_x86-64_windows_gnu.S");
        }
    } else {
        build.file("c/blake3_avx512_x86-64_unix.S");
    }

    // Older versions of Clang require these flags, even for assembly. See
    // https://github.com/BLAKE3-team/BLAKE3/issues/79.
    if !is_msvc {
        build.flag("-mavx512f");
        build.flag("-mavx512vl");
    }
    build.compile("blake3_avx512_assembly");
}

fn build_neon_c_intrinsics() {
    let mut build = new_build();
    // Note that blake3_neon.c normally depends on the blake3_portable.c
    // for the single-instance compression function, but we expose
    // portable.rs over FFI instead. See ffi_neon.rs.
    build.file("c/blake3_neon.c");
    // ARMv7 platforms that support NEON generally need the following
    // flags. AArch64 supports NEON by default and does not support -mpfu.
    if is_armv7() {
        build.flag("-mfpu=neon-vfpv4");
        build.flag("-mfloat-abi=hard");
    }
    build.compile("blake3_neon");
}

fn build_wasm32_simd() {
    assert!(is_wasm32());
    // No C code to compile here. Set the cfg flags that enable the Wasm SIMD.
    // The regular Cargo build will compile it.
    println!("cargo:rustc-cfg=blake3_wasm32_simd");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // As of Rust 1.80, unrecognized config names are warnings. Give Cargo all of our config names.
    let all_cfgs = [
        "blake3_sse2_ffi",
        "blake3_sse2_rust",
        "blake3_sse41_ffi",
        "blake3_sse41_rust",
        "blake3_avx2_ffi",
        "blake3_avx2_rust",
        "blake3_avx512_ffi",
        "blake3_neon",
        "blake3_wasm32_simd",
    ];
    for cfg_name in all_cfgs {
        // TODO: Switch this whole file to the new :: syntax when our MSRV reaches 1.77.
        // https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script
        println!("cargo:rustc-check-cfg=cfg({cfg_name}, values(none()))");
    }

    if is_pure() && is_neon() {
        panic!("It doesn't make sense to enable both \"pure\" and \"neon\".");
    }

    if is_no_neon() && is_neon() {
        panic!("It doesn't make sense to enable both \"no_neon\" and \"neon\".");
    }

    if is_x86_64() || is_x86_32() {
        let support = c_compiler_support();
        if is_x86_32() || should_prefer_intrinsics() || is_pure() || support == NoCompiler {
            build_sse2_sse41_avx2_rust_intrinsics();
        } else {
            // We assume that all C compilers can assemble SSE4.1 and AVX2. We
            // don't explicitly check for support.
            build_sse2_sse41_avx2_assembly();
        }

        if is_pure() || support == NoCompiler || support == NoAVX512 {
            // The binary will not include any AVX-512 code.
        } else if is_x86_32() || should_prefer_intrinsics() {
            build_avx512_c_intrinsics();
        } else {
            build_avx512_assembly();
        }
    }

    if is_neon() && is_big_endian() {
        panic!("The NEON implementation doesn't support big-endian ARM.")
    }

    if (is_arm() && is_neon())
        || (!is_no_neon() && !is_pure() && is_aarch64() && is_little_endian())
    {
        println!("cargo:rustc-cfg=blake3_neon");
        build_neon_c_intrinsics();
    }

    if is_wasm32() && is_wasm32_simd() {
        build_wasm32_simd();
    }

    // The `cc` crate doesn't automatically emit rerun-if directives for the
    // environment variables it supports, in particular for $CC. We expect to
    // do a lot of benchmarking across different compilers, so we explicitly
    // add the variables that we're likely to need.
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=CFLAGS");

    // Ditto for source files, though these shouldn't change as often.
    for file in std::fs::read_dir("c")? {
        println!(
            "cargo:rerun-if-changed={}",
            file?.path().to_str().expect("utf-8")
        );
    }

    // When compiling with clang-cl for windows, it adds .asm files to the root
    // which we need to delete so cargo doesn't get angry
    if is_windows_target() && !use_msvc_asm() {
        let _ = std::fs::remove_file("blake3_avx2_x86-64_windows_gnu.asm");
        let _ = std::fs::remove_file("blake3_avx512_x86-64_windows_gnu.asm");
        let _ = std::fs::remove_file("blake3_sse2_x86-64_windows_gnu.asm");
        let _ = std::fs::remove_file("blake3_sse41_x86-64_windows_gnu.asm");
    }

    Ok(())
}
