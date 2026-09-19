#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 0; }' | $CC --ld-path=$mold -dynamiclib \
  -xc - -o $t/libfoo.dylib -install_name $PWD/$t/libfoo.dylib
echo 'int main() {}' | $CC -c -xc - -o $t/a.o

$CC --ld-path=$mold $t/a.o $t/libfoo.dylib -Wl,-dead_strip_dylibs -o $t/exe
otool -L $t/exe > $t/libs
not grep -q libfoo.dylib $t/libs

$CC --ld-path=$mold $t/a.o -o $t/exe \
  -Wl,-needed_library,$t/libfoo.dylib,-dead_strip_dylibs
otool -L $t/exe | grep -q libfoo.dylib
$t/exe
