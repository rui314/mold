#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -B. -o $t/exe1 $t/a.o -Wl,-adhoc_codesign
otool -l $t/exe1 | grep -q LC_CODE_SIGNATURE
$t/exe1 | grep -Fq 'Hello world'

$CC --ld-path=$mold -B. -o $t/exe2 $t/a.o -Wl,-no_adhoc_codesign
otool -l $t/exe2 > $t/log2
! grep -q LC_CODE_SIGNATURE $t/log2 || false
grep -q LC_UUID $t/log2
! grep -q 'uuid 00000000-0000-0000-0000-000000000000' $t/log2 || false
