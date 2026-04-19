#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC $flags -
#include <stdio.h>

__attribute__((section(".fn1"))) void fn1() { printf(" fn1"); }
__attribute__((section(".fn2"))) void fn2() { printf(" fn2"); }

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -no-pie \
  -Wl,--section-order='=0x100000 EHDR PHDR =0x200000 .fn2 TEXT =0x300000 .fn1 DATA BSS RODATA'
$QEMU $t/exe1 | grep Hello

readelf -SW $t/exe1 | grep '\.fn2 .*00200000'
readelf -SW $t/exe1 | grep '\.fn1 .*00300000'


$CC -B. -o $t/exe2 $t/a.o -no-pie \
  -Wl,--section-order='=0x200000 EHDR RODATA =0x300000 PHDR =0x400000 .fn2 TEXT DATA BSS'

readelf -SW $t/exe2 | grep '\.fn2 .*00400000'
readelf -sW $t/exe2 | grep -E ': 0+200000\s.*\s__ehdr_start$'
readelf -W --segments $t/exe2 | grep -E 'PHDR\s.*0x0+300000\s'


$CC -B. -o $t/exe3 $t/a.o -no-pie \
  -Wl,--section-order='=0x200000 !ehdr_start EHDR %0x20 !rodata_start RODATA =0x300000 !phdr_start PHDR %4096 !phdr_end =0x400000 !text_start TEXT DATA BSS'

readelf -sW $t/exe3 > $t/log3
grep -E '\b0+200000 .* ehdr_start$' $t/log3
grep -E '\b0+200040 .* rodata_start$' $t/log3
grep -E '\b0+300000 .* phdr_start$' $t/log3
grep -E '\b0+301000 .* phdr_end$' $t/log3
grep -E '\b0+400000 .* text_start$' $t/log3

not $CC -B. -o $t/exe4 $t/a.o -no-pie \
  -Wl,--section-order='=0x800000 EHDR PHDR =0x200000 .fn2 TEXT =0x300000 .fn1 DATA BSS RODATA' |&
  grep 'address goes backward'
