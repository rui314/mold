#!/bin/bash
source "$(dirname "$0")"/common.inc

# $LD_DYLIB_ARCH_FALLBACK, "<arch>:<other>", has a link for <arch> take
# a dylib of <other> - thin, or a fat file's slice - where it has none
# of its own architecture; objects, archives and stubs stay out. A value
# for another architecture does nothing.
[ $ARCH = arm64 ] && other=x86_64 || other=arm64
echo 'int foo(void) { return 1; }' | cc -arch $other -dynamiclib -o $t/libx.dylib -xc - \
  -install_name /x/libx.dylib
lipo -create $t/libx.dylib -output $t/libfat.dylib
echo 'int foo(void) { return 1; }' | cc -arch $other -o $t/x.o -c -xc -
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/a.o -c -xc -

for lib in libx.dylib libfat.dylib; do
  LD_DYLIB_ARCH_FALLBACK=$ARCH:$other $CC --ld-path=$mold -o $t/exe $t/a.o $t/$lib 2> $t/log
  not grep -q warning $t/log
  otool -L $t/exe | grep -q /x/libx.dylib
done

not env LD_DYLIB_ARCH_FALLBACK=$other:$ARCH $CC --ld-path=$mold -o $t/exe \
  $t/a.o $t/libfat.dylib 2> $t/log
grep -q "ignoring file '$t/libfat.dylib': fat file missing arch '$ARCH', file has '$other'" $t/log

not env LD_DYLIB_ARCH_FALLBACK=$ARCH:$other $CC --ld-path=$mold -o $t/exe \
  $t/a.o $t/x.o 2> $t/log
grep -q "ignoring file '$t/x.o': found architecture '$other', required architecture '$ARCH'" $t/log
