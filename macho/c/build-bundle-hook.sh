#!/bin/sh
# Builds the objects of the hook for the classes of mergeable libraries
# that mold embeds (see bundle-hook.c and ../bundle_hook.rs), so that
# building mold needs no C compiler. Run it on macOS after changing
# bundle-hook.c, and commit what it writes.
#
# The objects are for the oldest macOS releases clang builds for: an
# object for a newer one than an image would make ld-prime warn (mold
# doesn't for its own, either way).
set -e
cd "$(dirname "$0")"
for target in arm64-apple-macos11.0 x86_64-apple-macos10.13; do
  xcrun clang -target $target -O2 -Wall -Werror -fno-stack-protector \
    -c bundle-hook.c -o bundle-hook-${target%%-*}.o
done
