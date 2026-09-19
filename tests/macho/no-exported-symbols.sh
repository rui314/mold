#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int foo() { return 42; } int main() { return foo() != 42; }' |
  $CC -c -xc - -o $t/a.o

for kind in '' -dynamiclib; do
  $CC --ld-path=$mold $t/a.o $kind -Wl,-no_exported_symbols -o $t/out
  nm -gU $t/out > $t/syms
  not grep -q _ $t/syms
  dyld_info -exports $t/out > $t/exports
  not grep -Eq '_foo|_main|__mh_execute_header' $t/exports
done

$CC --ld-path=$mold $t/a.o -Wl,-no_exported_symbols,-dead_strip -o $t/exe
$t/exe
$CC --ld-path=$mold $t/a.o -dynamiclib \
  -Wl,-no_exported_symbols,-dead_strip -o $t/libfoo.dylib
nm $t/libfoo.dylib > $t/syms
not grep -q _foo $t/syms

$mold -arch $ARCH -r $t/a.o -no_exported_symbols -o $t/b.o
nm -m $t/b.o | grep -q 'non-external.*_foo'
nm -gU $t/b.o > $t/syms
not grep -q _ $t/syms

for opt in -exported_symbol -unexported_symbol; do
  not $CC --ld-path=$mold $t/a.o -Wl,-no_exported_symbols,$opt,_foo -o $t/exe 2> $t/log
  grep -q 'cannot be used' $t/log
done
