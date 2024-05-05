#!/bin/bash
. $(dirname $0)/common.inc

# Executables built with `-z rodynamic` crash on qemu-user for
# PowerPC for some reason.
[[ $MACHINE = ppc* ]] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -WS $t/exe1 | grep -q '\.dynamic.* WA '
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,rodynamic
readelf -WS $t/exe2 | grep -q '\.dynamic.* A '
$QEMU $t/exe2 | grep -q 'Hello world'
