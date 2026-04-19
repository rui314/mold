#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -fPIC -xc -
#include <stdio.h>
int main() {
  puts("Hello world");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-emit-relocs
$QEMU $t/exe | grep 'Hello world'

readelf -S $t/exe | grep -E 'rela?\.text'
