#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static-pie || skip
[ $MACHINE = aarch64 ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe2 $t/a.o -static-pie
$QEMU $t/exe2 | grep -q 'Hello world'
