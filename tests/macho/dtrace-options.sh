#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes -dtrace's path without opening it, in a -r link too,
# and -no_dtrace_dof changes nothing for an image without USDT probe
# sites. -dtrace needs its argument.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o
mv $t/exe $t/exe1
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dtrace,$t/nonexistent.d 2> $t/log
[ ! -s $t/log ]
cmp $t/exe $t/exe1
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_dtrace_dof 2> $t/log
[ ! -s $t/log ]
cmp $t/exe $t/exe1

$mold -r -arch $ARCH -o $t/r1.o $t/a.o
$mold -r -arch $ARCH -o $t/r2.o $t/a.o -dtrace $t/nonexistent.d -no_dtrace_dof
cmp $t/r1.o $t/r2.o

not $mold -arch $ARCH -o $t/exe $t/a.o -dtrace 2> $t/log
grep -q -- '-dtrace.*missing' $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -dtrace '' 2> $t/log
grep -q -- '-dtrace.*missing' $t/log
