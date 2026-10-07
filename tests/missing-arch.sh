#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without -arch, the first object file on the command line names the
# target. Archives, dylibs, stubs and universal files don't count; with
# nothing else, there is no target. A file that can't be opened stops
# the search, in words that name no input.
[ $ARCH = arm64 ] && other=x86_64 || other=arm64
echo 'int main() { return 0; }' > $t/a.c
$CC -c $t/a.c -o $t/a.o
${CC/$ARCH/$other} -c $t/a.c -o $t/other.o
lipo -create $t/a.o -output $t/fat.o
rm -f $t/liba.a
ar rcs $t/liba.a $t/a.o
$CC -shared -o $t/libfoo.dylib -xc /dev/null

link() {
  $mold -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -o $t/exe "$@" $SDK/usr/lib/libSystem.tbd
}

for input in $t/liba.a $t/libfoo.dylib $t/fat.o; do
  not link $input 2> $t/log
  grep -q ': Missing -arch option$' $t/log
done
not $mold -r -o $t/r.o $t/liba.a 2> $t/log
grep -q 'Missing -arch option' $t/log

link $t/liba.a $t/libfoo.dylib $t/a.o $t/other.o 2> $t/log
grep -qF "ignoring file '$t/other.o': found architecture '$other', required architecture '$ARCH'" $t/log
otool -hv $t/exe > $t/hdr
grep -q " $(echo $ARCH | tr a-z A-Z) " $t/hdr

not link $t/libfoo.dylib $t/nonexistent.o $t/a.o 2> $t/log
grep -qF "$t/nonexistent.o" $t/log
grep -q 'No such file or directory' $t/log
