#!/bin/bash
source "$(dirname "$0")"/common.inc

mkdir -p $t/Foo.framework
echo 'int foo() { return 42; }' | $CC --ld-path=$mold -dynamiclib \
  -xc - -o $t/Foo.framework/Foo -install_name $PWD/$t/Foo.framework/Foo

echo 'int wrapper() { return 0; }' | $CC --ld-path=$mold -dynamiclib \
  -xc - -o $t/libouter.dylib -install_name $PWD/$t/libouter.dylib \
  -F$t -Wl,-reexport_framework,Foo
otool -l $t/libouter.dylib | grep -q LC_REEXPORT_DYLIB

echo 'int foo(); int main() { return foo() != 42; }' | $CC -c -xc - -o $t/a.o
$CC --ld-path=$mold $t/a.o $t/libouter.dylib -o $t/exe
$t/exe
