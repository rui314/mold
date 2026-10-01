#!/bin/bash
source "$(dirname "$0")"/common.inc

# -l looks for lib<name>.so too, as a dylib: after lib<name>.tbd and
# lib<name>.dylib and before lib<name>.a in each directory. Under
# -search_dylibs_first one anywhere beats an archive anywhere, and the
# options that take a dylib only (-upward-l, -reexport-l) find one.
sdk=$(xcrun --show-sdk-path)
mkdir -p $t/a $t/b $t/c $t/d

echo 'int foo(void) { return 0; }' > $t/foo.c
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/main.o -c -xc -
$CC -o $t/foo.o -c $t/foo.c

dylib() {
  $CC -shared -o $t$1 $t/foo.c -install_name $1
}
dylib /a/libx.so
dylib /a/libx.dylib
dylib /b/liby.so
ar rcs $t/b/liby.a $t/foo.o
ar rcs $t/c/libz.a $t/foo.o
dylib /d/libz.so

link() {
  $mold -arch $ARCH -platform_version macos 13.0 13.0 $t/main.o \
    $sdk/usr/lib/libSystem.tbd "$@" 2> /dev/null
}

link -o $t/exe1 -L$t/a -lx
otool -L $t/exe1 | grep -q /a/libx.dylib

link -o $t/exe2 -L$t/b -ly
otool -L $t/exe2 | grep -q /b/liby.so

link -o $t/exe3 -L$t/c -L$t/d -lz
otool -L $t/exe3 > $t/libs3
not grep -q libz $t/libs3

link -o $t/exe4 -L$t/c -L$t/d -lz -search_dylibs_first
otool -L $t/exe4 | grep -q /d/libz.so

link -o $t/lib5.dylib -dylib -L$t/b -upward-ly
otool -L $t/lib5.dylib | grep -q '/b/liby.so .*upward'
