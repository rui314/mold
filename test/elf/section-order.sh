#!/bin/bash
. $(dirname $0)/common.inc

# qemu crashes if the ELF header is not mapped to memory
[ -z "$QEMU" ] || skip

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC $flags -
#include <stdio.h>

__attribute__((section(".fn1"))) void fn1() { printf(" fn1"); }
__attribute__((section(".fn2"))) void fn2() { printf(" fn2"); }

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -no-pie \
  -Wl,--section-order='.fn2=0x100000 #text .fn1=0x200000 #data #rodata'
$QEMU $t/exe1 | grep -q Hello

readelf -SW $t/exe1 | grep -q '\.fn2 .*00100000'
readelf -SW $t/exe1 | grep -q '\.fn1 .*00200000'
readelf -sw $t/exe1 | grep -Eq ': 0+\s.*\s__ehdr_start$'

$CC -B. -o $t/exe2 $t/a.o -no-pie \
  -Wl,--section-order='#ehdr=0x200000 #rodata #phdr=0x300000 .fn2=0x400000 #text #data'
$QEMU $t/exe2 | grep -q Hello

readelf -SW $t/exe2 | grep -q '\.fn2 .*00400000'
readelf -sW $t/exe2 | grep -Eq ': 0+200000\s.*\s__ehdr_start$'
readelf -W --segments $t/exe2 | grep -Eq 'PHDR\s.*0x0+300000\s'
