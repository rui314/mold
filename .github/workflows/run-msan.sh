#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."

# MSan requires instrumented Rust, std, and native dependencies:
# https://doc.rust-lang.org/unstable-book/compiler-flags/sanitizer.html#memorysanitizer
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/msan}"
export RUSTFLAGS='-Zsanitizer=memory -Zsanitizer-memory-track-origins=2'
export RUSTDOCFLAGS="$RUSTFLAGS"
export CC_x86_64_unknown_linux_gnu=clang
export CFLAGS_x86_64_unknown_linux_gnu='-fsanitize=memory -fsanitize-memory-track-origins=2 -fno-omit-frame-pointer'
export LIBZ_SYS_STATIC=1
export MSAN_OPTIONS=halt_on_error=1

# The instrumented linker is too slow unoptimized for the heaviest tests
# to finish within the per-test timeout.
export CARGO_PROFILE_DEV_OPT_LEVEL=1

# Use instrumentable implementations instead of BLAKE3/Zstd assembly, and
# select mold as well as mold-cli so Cargo enables its dependency features.
args=(
  --locked -Zbuild-std --target x86_64-unknown-linux-gnu
  --package mold --package mold-cli --no-default-features
  --features mold-cli/system-allocator,mold-cli/x86_64,blake3/pure,zstd/no_asm
  --test integration
)
cargo +nightly test "${args[@]}" --no-run

# The preload wrapper runs inside uninstrumented compiler processes, not
# inside mold. Rebuild it without MSan so those processes can load it.
clang -shared -fPIC -o "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/debug/mold-wrapper.so" \
  c/mold-wrapper.c -ldl

cargo +nightly test "${args[@]}" -- --native --test-threads 1 --timeout 60 "$@"
