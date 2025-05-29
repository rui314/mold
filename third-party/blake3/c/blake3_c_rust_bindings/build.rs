use std::env;

fn defined(var: &str) -> bool {
    env::var_os(var).is_some()
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

fn is_armv7() -> bool {
    target_components()[0] == "armv7"
}

fn is_aarch64() -> bool {
    target_components()[0] == "aarch64"
}

// Windows targets may be using the MSVC toolchain or the GNU toolchain. The
// right compiler flags to use depend on the toolchain. (And we don't want to
// use flag_if_supported, because we don't want features to be silently
// disabled by old compilers.)
fn is_windows_msvc() -> bool {
    // Some targets are only two components long, so check in steps.
    target_components()[1] == "pc"
        && target_components()[2] == "windows"
        && target_components()[3] == "msvc"
}

fn new_build() -> cc::Build {
    let mut build = cc::Build::new();
    if !is_windows_msvc() {
        build.flag("-std=c11");
    }
    build
}

fn new_cpp_build() -> cc::Build {
    let mut build = cc::Build::new();
    build.cpp(true);
    if is_windows_msvc() {
        build.flag("/std:c++20");
        build.flag("/EHs-c-");
        build.flag("/GR-");
    } else {
        build.flag("-std=c++20");
        build.flag("-fno-exceptions");
        build.flag("-fno-rtti");
    }
    build
}

fn c_dir_path(filename: &str) -> String {
    // The `cross` tool doesn't support reading files in parent directories. As a hacky workaround
    // in `cross_test.sh`, we move the c/ directory around and set BLAKE3_C_DIR_OVERRIDE. Regular
    // building and testing doesn't require this.
    if let Ok(c_dir_override) = env::var("BLAKE3_C_DIR_OVERRIDE") {
        c_dir_override + "/" + filename
    } else {
        "../".to_string() + filename
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut base_build = new_build();
    base_build.file(c_dir_path("blake3.c"));
    base_build.file(c_dir_path("blake3_dispatch.c"));
    base_build.file(c_dir_path("blake3_portable.c"));
    if cfg!(feature = "tbb") {
        base_build.define("BLAKE3_USE_TBB", "1");
    }
    base_build.compile("blake3_base");

    if cfg!(feature = "tbb") {
        let mut tbb_build = new_cpp_build();
        tbb_build.define("BLAKE3_USE_TBB", "1");
        tbb_build.file(c_dir_path("blake3_tbb.cpp"));
        tbb_build.compile("blake3_tbb");
        println!("cargo::rustc-link-lib=tbb");
    }

    if is_x86_64() && !defined("CARGO_FEATURE_PREFER_INTRINSICS") {
        // On 64-bit, use the assembly implementations, unless the
        // "prefer_intrinsics" feature is enabled.
        if is_windows_target() {
            if use_msvc_asm() {
                let mut build = new_build();
                build.file(c_dir_path("blake3_sse2_x86-64_windows_msvc.asm"));
                build.file(c_dir_path("blake3_sse41_x86-64_windows_msvc.asm"));
                build.file(c_dir_path("blake3_avx2_x86-64_windows_msvc.asm"));
                build.file(c_dir_path("blake3_avx512_x86-64_windows_msvc.asm"));
                build.compile("blake3_asm");
            } else {
                let mut build = new_build();
                build.file(c_dir_path("blake3_sse2_x86-64_windows_gnu.S"));
                build.file(c_dir_path("blake3_sse41_x86-64_windows_gnu.S"));
                build.file(c_dir_path("blake3_avx2_x86-64_windows_gnu.S"));
                build.file(c_dir_path("blake3_avx512_x86-64_windows_gnu.S"));
                build.compile("blake3_asm");
            }
        } else {
            // All non-Windows implementations are assumed to support
            // Linux-style assembly. These files do contain a small
            // explicit workaround for macOS also.
            let mut build = new_build();
            build.file(c_dir_path("blake3_sse2_x86-64_unix.S"));
            build.file(c_dir_path("blake3_sse41_x86-64_unix.S"));
            build.file(c_dir_path("blake3_avx2_x86-64_unix.S"));
            build.file(c_dir_path("blake3_avx512_x86-64_unix.S"));
            build.compile("blake3_asm");
        }
    } else if is_x86_64() || is_x86_32() {
        // Assembly implementations are only for 64-bit. On 32-bit, or if
        // the "prefer_intrinsics" feature is enabled, use the
        // intrinsics-based C implementations. These each need to be
        // compiled separately, with the corresponding instruction set
        // extension explicitly enabled in the compiler.

        let mut sse2_build = new_build();
        sse2_build.file(c_dir_path("blake3_sse2.c"));
        if is_windows_msvc() {
            // /arch:SSE2 is the default on x86 and undefined on x86_64:
            // https://docs.microsoft.com/en-us/cpp/build/reference/arch-x86
            // It also includes SSE4.1 intrinsics:
            // https://stackoverflow.com/a/32183222/823869
        } else {
            sse2_build.flag("-msse2");
        }
        sse2_build.compile("blake3_sse2");

        let mut sse41_build = new_build();
        sse41_build.file(c_dir_path("blake3_sse41.c"));
        if is_windows_msvc() {
            // /arch:SSE2 is the default on x86 and undefined on x86_64:
            // https://docs.microsoft.com/en-us/cpp/build/reference/arch-x86
            // It also includes SSE4.1 intrinsics:
            // https://stackoverflow.com/a/32183222/823869
        } else {
            sse41_build.flag("-msse4.1");
        }
        sse41_build.compile("blake3_sse41");

        let mut avx2_build = new_build();
        avx2_build.file(c_dir_path("blake3_avx2.c"));
        if is_windows_msvc() {
            avx2_build.flag("/arch:AVX2");
        } else {
            avx2_build.flag("-mavx2");
        }
        avx2_build.compile("blake3_avx2");

        let mut avx512_build = new_build();
        avx512_build.file(c_dir_path("blake3_avx512.c"));
        if is_windows_msvc() {
            // Note that a lot of versions of MSVC don't support /arch:AVX512,
            // and they'll discard it with a warning, hopefully leading to a
            // build error.
            avx512_build.flag("/arch:AVX512");
        } else {
            avx512_build.flag("-mavx512f");
            avx512_build.flag("-mavx512vl");
        }
        avx512_build.compile("blake3_avx512");
    }

    // We only build NEON code here if
    // 1) it's requested
    // and 2) the root crate is not already building it.
    // The only time this will really happen is if you build this
    // crate by hand with the "neon" feature for some reason.
    //
    // In addition, 3) if the target is aarch64, NEON is on by default.
    if defined("CARGO_FEATURE_NEON") || is_aarch64() {
        let mut neon_build = new_build();
        neon_build.file(c_dir_path("blake3_neon.c"));
        // ARMv7 platforms that support NEON generally need the following
        // flags. AArch64 supports NEON by default and does not support -mpfu.
        if is_armv7() {
            neon_build.flag("-mfpu=neon-vfpv4");
            neon_build.flag("-mfloat-abi=hard");
        }
        neon_build.compile("blake3_neon");
    }

    // The `cc` crate does not automatically emit rerun-if directives for the
    // environment variables it supports, in particular for $CC. We expect to
    // do a lot of benchmarking across different compilers, so we explicitly
    // add the variables that we're likely to need.
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=CFLAGS");

    // Ditto for source files, though these shouldn't change as often. `ignore::Walk` respects
    // .gitignore, so this doesn't traverse target/.
    for result in ignore::Walk::new("..") {
        let result = result?;
        let path = result.path();
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
        }
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
