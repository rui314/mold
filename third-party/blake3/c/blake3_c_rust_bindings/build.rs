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

fn is_windows_gnu() -> bool {
    // Some targets are only two components long, so check in steps.
    target_components()[1] == "pc"
        && target_components()[2] == "windows"
        && target_components()[3] == "gnu"
}

fn new_build() -> cc::Build {
    let mut build = cc::Build::new();
    if !is_windows_msvc() {
        build.flag("-std=c11");
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
    base_build.compile("blake3_base");

    if is_x86_64() && !defined("CARGO_FEATURE_PREFER_INTRINSICS") {
        // On 64-bit, use the assembly implementations, unless the
        // "prefer_intrinsics" feature is enabled.
        if is_windows_msvc() {
            let mut build = new_build();
            build.file(c_dir_path("blake3_sse2_x86-64_windows_msvc.asm"));
            build.file(c_dir_path("blake3_sse41_x86-64_windows_msvc.asm"));
            build.file(c_dir_path("blake3_avx2_x86-64_windows_msvc.asm"));
            build.file(c_dir_path("blake3_avx512_x86-64_windows_msvc.asm"));
            build.compile("blake3_asm");
        } else if is_windows_gnu() {
            let mut build = new_build();
            build.file(c_dir_path("blake3_sse2_x86-64_windows_gnu.S"));
            build.file(c_dir_path("blake3_sse41_x86-64_windows_gnu.S"));
            build.file(c_dir_path("blake3_avx2_x86-64_windows_gnu.S"));
            build.file(c_dir_path("blake3_avx512_x86-64_windows_gnu.S"));
            build.compile("blake3_asm");
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

    // Ditto for source files, though these shouldn't change as often.
    for file in std::fs::read_dir("..")? {
        println!(
            "cargo:rerun-if-changed={}",
            file?.path().to_str().expect("utf-8")
        );
    }

    Ok(())
}
