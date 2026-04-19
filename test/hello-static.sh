#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -static
$QEMU $t/exe | grep 'Hello world'
