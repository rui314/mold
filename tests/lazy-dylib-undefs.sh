#!/bin/bash
source "$(dirname "$0")"/common.inc

# The helpers through which an image calls a lazy dylib's symbols
# (from macOS 27) have __dyld_lazy_load load the dylib: a link that
# uses such a symbol needs a loaded dylib (libSystem) that exports it,
# as any import does. A kext and a -static image link no dylib, so the
# lazy dylib's symbols stay undefined there: dynamically looked up in a
# kext, an error in a -static image, which has no dyld. A lazy dylib
# has no load command, so it doesn't make up for a missing libSystem.
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc - -mmacosx-version-min=27.0
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib \
  -mmacosx-version-min=27.0
echo 'int bar(void) { return 4; }' | $CC -o $t/bar.o -c -xc - -mmacosx-version-min=27.0
$CC -o $t/libbar.dylib -shared $t/bar.o -Wl,-install_name,@rpath/libbar.dylib \
  -mmacosx-version-min=27.0
echo 'int foo(void); int kmod_start(void) { return foo(); }' | \
  $CC -o $t/k.o -c -xc - -mmacosx-version-min=27.0
echo 'int foo(void); int main(void) { return foo(); }' | \
  $CC -o $t/s.o -c -xc - -mmacosx-version-min=27.0

link() {
  $mold -arch $ARCH -platform_version macos 27.0 27.0 -syslibroot "$(xcrun --show-sdk-path)" "$@"
}

link -kext -o $t/k.kext $t/k.o -lazy_library $t/libfoo.dylib 2> /dev/null
nm -m $t/k.kext | grep -q '(undefined) external _foo (dynamically looked up)'

not link -static -e _main -o $t/s.out $t/s.o -lazy_library $t/libfoo.dylib 2> $t/log
grep -q '_foo' $t/log

# libbar exports no __dyld_lazy_load.
not link -o $t/n.out $t/s.o $t/libbar.dylib -lazy_library $t/libfoo.dylib 2> $t/log
grep -q '__dyld_lazy_load' $t/log

not link -dylib -o $t/d.dylib $t/s.o -lazy_library $t/libfoo.dylib \
  -undefined dynamic_lookup 2> $t/log
grep -q 'dynamic executables or dylibs must link with libSystem.dylib' $t/log
