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
