#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-z,relro,-z,lazy
$QEMU $t/exe1 | grep 'Hello world'
readelf --segments -W $t/exe1 | grep -w GNU_RELRO

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,relro,-z,now
$QEMU $t/exe2 | grep 'Hello world'
readelf --segments -W $t/exe2 | grep -w GNU_RELRO

$CC -B. -o $t/exe3 $t/a.o -Wl,-z,norelro
$QEMU $t/exe3 | grep 'Hello world'
readelf --segments -W $t/exe3 | not grep -w GNU_RELRO
