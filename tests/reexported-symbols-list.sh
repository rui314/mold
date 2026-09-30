#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 42; } int bar() { return 7; }' |
  $CC --ld-path=$mold -dynamiclib -xc - -o $t/libinner.dylib \
    -install_name $PWD/$t/libinner.dylib
echo 'int wrapper() { return 0; }' | $CC -c -xc - -o $t/a.o
echo '_foo # selected export' > $t/list

$CC --ld-path=$mold -dynamiclib $t/a.o $t/libinner.dylib -o $t/libouter.dylib \
  -install_name $PWD/$t/libouter.dylib \
  -Wl,-reexported_symbols_list,$t/list,-dead_strip,-dead_strip_dylibs
dyld_info -exports $t/libouter.dylib > $t/exports
grep -q 're-export.*_foo' $t/exports
grep -q _wrapper $t/exports
not grep -q _bar $t/exports
otool -l $t/libouter.dylib > $t/loads
not grep -q LC_REEXPORT_DYLIB $t/loads

echo 'int foo(); int main() { return foo() != 42; }' | $CC -c -xc - -o $t/main.o
$CC --ld-path=$mold $t/main.o $t/libouter.dylib -o $t/exe
$t/exe

echo _missing > $t/list
not $CC --ld-path=$mold -dynamiclib $t/a.o $t/libinner.dylib \
  -Wl,-reexported_symbols_list,$t/list,-dead_strip -o $t/libbad.dylib 2> $t/log
grep -q _missing $t/log

# Only a dylib re-exports symbols: ld-prime refuses the list, empty or
# not, for any other output, though an executable may re-export a
# library whole (LC_REEXPORT_DYLIB).
: > $t/empty
msg='-reexported_symbols_list can only used used when created dynamic libraries'
not $CC --ld-path=$mold $t/main.o $t/libouter.dylib -o $t/exe2 \
  -Wl,-reexported_symbols_list,$t/empty 2> $t/log2
grep -qF -- "$msg" $t/log2
echo _foo > $t/list
not $CC --ld-path=$mold -bundle $t/a.o $t/libinner.dylib -o $t/b.bundle \
  -Wl,-reexported_symbols_list,$t/list 2> $t/log3
grep -qF -- "$msg" $t/log3
not $mold -arch $ARCH -r $t/a.o -o $t/r.o -reexported_symbols_list $t/list 2> $t/log4
grep -qF -- "$msg" $t/log4
$CC --ld-path=$mold $t/main.o -Wl,-reexport_library,$t/libouter.dylib -o $t/exe3
otool -l $t/exe3 | grep -q LC_REEXPORT_DYLIB
