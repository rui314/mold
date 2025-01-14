#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>

void hello() {
  puts("Hello world");
}

__attribute__((section(".text")))
void (*p)() = hello;

int main() {
  p();
}
EOF

$CC -B. -o $t/exe1 $t/a.o -pie
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -o $t/exe2 $t/a.o -pie -Wl,-z,pack-relative-relocs 2> /dev/null || skip
readelf -WS $t/exe2 | grep -Fq .relr.dyn || skip
$QEMU $t/exe2 2> /dev/null | grep -q 'Hello world' || skip

$CC -B. -o $t/exe3 $t/a.o -pie -Wl,-z,pack-relative-relocs
$QEMU $t/exe3 | grep -q 'Hello world'
