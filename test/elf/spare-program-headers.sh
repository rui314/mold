#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o
$QEMU $t/exe1 | grep -q 'Hello world'
[ "$(readelf -Wl $t/exe1 | grep NULL | wc -l)" -eq 0 ]

$CC -B. -o $t/exe2 $t/a.o -Wl,--spare-program-headers=0
$QEMU $t/exe2 | grep -q 'Hello world'
[ "$(readelf -Wl $t/exe2 | grep NULL | wc -l)" -eq 0 ]

$CC -B. -o $t/exe3 $t/a.o -Wl,--spare-program-headers=1
$QEMU $t/exe3 | grep -q 'Hello world'
[ "$(readelf -Wl $t/exe3 | grep NULL | wc -l)" -eq 1 ]

$CC -B. -o $t/exe4 $t/a.o -Wl,--spare-program-headers=5
$QEMU $t/exe4 | grep -q 'Hello world'
[ "$(readelf -Wl $t/exe4 | grep NULL | wc -l)" -eq 5 ]
