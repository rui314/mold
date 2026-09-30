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
# The hidden header symbol stays in the symbol table as a local.
nm -m $t/exe > $t/nm
grep -q 'non-external (was a private external) __mh_execute_header' $t/nm
$CC --ld-path=$mold $t/a.o -dynamiclib \
  -Wl,-no_exported_symbols,-dead_strip -o $t/libfoo.dylib
nm $t/libfoo.dylib > $t/syms
not grep -q _foo $t/syms

$mold -arch $ARCH -r $t/a.o -no_exported_symbols -o $t/b.o
nm -m $t/b.o | grep 'non-external.*_foo'
nm -gU $t/b.o > $t/syms
not grep -q _ $t/syms

for opt in -exported_symbol -unexported_symbol; do
  not $CC --ld-path=$mold $t/a.o -Wl,-no_exported_symbols,$opt,_foo -o $t/exe 2> $t/log
  grep -q 'cannot be used' $t/log
done

# An export list and an unexport list exclude each other too; ld64's
# message names the option that comes second.
echo _foo > $t/list
not $CC --ld-path=$mold $t/a.o -Wl,-exported_symbols_list,$t/list \
  -Wl,-unexported_symbols_list,$t/list -o $t/exe 2> $t/log2
grep -q -- '-unexported_symbols_list: -exported_symbol\*, -unexported_symbol\* and -no_exported_symbols cannot be used together' $t/log2
not $CC --ld-path=$mold $t/a.o -Wl,-exported_symbol,_foo -Wl,-unexported_symbol,_bar \
  -o $t/exe 2> $t/log3
grep -q -- '-unexported_symbol cannot be used with -exported_symbol\*' $t/log3
