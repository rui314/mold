#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello ");
  puts("world");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,nosectionheader
$QEMU $t/exe | grep -q 'Hello world'

readelf -h $t/exe 2>&1 | grep -Eq 'Size of section headers:\s+0 '
