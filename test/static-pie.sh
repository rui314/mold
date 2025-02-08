#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static-pie || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -static-pie
$QEMU $t/exe1 | grep 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -static-pie -Wl,--no-relax
$QEMU $t/exe2 | grep 'Hello world'
