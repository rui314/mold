#!/bin/bash
source "$(dirname "$0")"/common.inc

# A link that names a lazy dylib (from macOS 27) wants __dyld_lazy_load
# as ld-prime's "<lazy-load-undefs>" does: an ordinary reference, not
# one the command line insists on. A kext, which links no dylib, takes
# it as it takes any undefined symbol, dynamically looked up; a -static
# image, which has no dyld, can't. A lazy dylib has no load command, so
# it doesn't make up for a missing libSystem.
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc - -mmacosx-version-min=27.0
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib \
  -mmacosx-version-min=27.0
echo 'int foo(void); int kmod_start(void) { return foo(); }' | \
  $CC -o $t/k.o -c -xc - -mmacosx-version-min=27.0
echo 'int main(void) { return 0; }' | $CC -o $t/s.o -c -xc - -mmacosx-version-min=27.0

link() {
  $mold -arch $ARCH -platform_version macos 27.0 27.0 -syslibroot "$(xcrun --show-sdk-path)" "$@"
}

link -kext -o $t/k.kext $t/k.o -lazy_library $t/libfoo.dylib 2> /dev/null
nm -m $t/k.kext | grep -q '(undefined) external __dyld_lazy_load (dynamically looked up)'

not link -static -e _main -o $t/s.out $t/s.o -lazy_library $t/libfoo.dylib 2> $t/log
grep -q '__dyld_lazy_load' $t/log
grep -q '<lazy-load-undefs>' $t/log

not link -dylib -o $t/d.dylib $t/s.o -lazy_library $t/libfoo.dylib \
  -undefined dynamic_lookup 2> $t/log
grep -q 'dynamic executables or dylibs must link with libSystem.dylib' $t/log
