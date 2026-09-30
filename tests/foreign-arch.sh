#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime ignores an input built for another architecture, with a
# warning: an object (an archive member too, needed or not) for another
# one than exactly the link's (an arm64e object in an arm64 link), a
# dylib for another CPU type, a universal file with no slice for the
# link. The link goes on without it.
[ $ARCH = arm64 ] && other=x86_64 || other=arm64
echo 'int foo() { return 1; }' > $t/foo.c
cc -arch $other -c $t/foo.c -o $t/other.o
cc -arch $other -shared $t/foo.c -o $t/libother.dylib
rm -f $t/libother.a
ar rcs $t/libother.a $t/other.o
lipo -create $t/libother.dylib -output $t/libfat.dylib

echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o $t/other.o $t/libother.dylib $t/libother.a \
  $t/libfat.dylib 2> $t/log
grep -qF "warning: ignoring file '$t/other.o': found architecture '$other', required architecture '$ARCH'" $t/log
grep -qF "warning: ignoring file '$t/libother.dylib': found architecture '$other', required architecture '$ARCH'" $t/log
grep -qF "warning: ignoring file '$t/libother.a(other.o)': found architecture '$other', required architecture '$ARCH'" $t/log
grep -qF "warning: ignoring file '$t/libfat.dylib': fat file missing arch '$ARCH', file has '$other'" $t/log
$t/exe
otool -L $t/exe > $t/libs
not grep -q libother $t/libs

# A symbol only the ignored dylib defines stays undefined.
echo 'int foo(); int main() { return foo(); }' | $CC -o $t/b.o -c -xc -
not $CC --ld-path=$mold -o $t/exe2 $t/b.o $t/libother.dylib 2> $t/log2
grep -q '_foo' $t/log2

if [ $ARCH = arm64 ]; then
  cc -arch arm64e -c $t/foo.c -o $t/e.o
  $CC --ld-path=$mold -o $t/exe3 $t/a.o $t/e.o 2> $t/log3
  grep -qF "warning: ignoring file '$t/e.o': found architecture 'arm64e', required architecture 'arm64'" $t/log3
  cc -arch arm64e -shared $t/foo.c -o $t/libe.dylib
  $CC --ld-path=$mold -o $t/exe4 $t/b.o $t/libe.dylib 2> $t/log4
  not grep -q 'ignoring file' $t/log4
fi
