#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -no-pie -o $t/exe1 $t/a.o -Wl,--physical-image-base=0x200000 -Wl,--physical-image-base=0x8000000
$QEMU $t/exe1 | grep -q 'Hello world'
readelf -W --segments $t/exe1 | grep -Eq 'LOAD\s+0x000000 0x0*200000 0x0*8000000'
