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

$CC -o $t/exe1 $t/a.o -pie -Wl,-z,notext
$QEMU $t/exe1 || skip

$CC -B. -o $t/exe2 $t/a.o -pie
$QEMU $t/exe2 | grep 'Hello world'

$CC -o $t/exe3 $t/a.o -pie -Wl,-z,notext -Wl,-z,pack-relative-relocs 2> /dev/null || skip
readelf -WS $t/exe3 | grep -F .relr.dyn || skip
$QEMU $t/exe3 2> /dev/null | grep 'Hello world' || skip

$CC -B. -o $t/exe4 $t/a.o -pie -Wl,-z,pack-relative-relocs
$QEMU $t/exe4 | grep 'Hello world'
