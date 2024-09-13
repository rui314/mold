#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -o $t/exe1 $t/a.o -pie -Wl,-z,pack-relative-relocs 2> /dev/null || skip
$QEMU $t/exe1 2> /dev/null | grep -q Hello || skip

$CC -B. -o $t/exe2 $t/a.o -pie -Wl,-z,pack-relative-relocs
$QEMU $t/exe2 | grep -q Hello

readelf --dynamic $t/exe2 > $t/log2
grep -wq RELR $t/log2
grep -wq RELRSZ $t/log2
grep -wq RELRENT $t/log2
