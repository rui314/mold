#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello ");
  puts("world");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,now
$QEMU $t/exe | grep 'Hello world'
