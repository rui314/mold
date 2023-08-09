#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -pie -Wl,-z,pack-relative-relocs

readelf -W -V $t/exe > $t/log
grep -Fq GLIBC_2. $t/log || skip

grep -q GLIBC_ABI_DT_RELR $t/log
