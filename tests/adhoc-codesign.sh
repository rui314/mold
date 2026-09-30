#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -B. -o $t/exe1 $t/a.o -Wl,-adhoc_codesign
otool -l $t/exe1 | grep LC_CODE_SIGNATURE
$t/exe1 | grep -F 'Hello world'

$CC --ld-path=$mold -B. -o $t/exe2 $t/a.o -Wl,-no_adhoc_codesign
otool -l $t/exe2 > $t/log2
! grep -q LC_CODE_SIGNATURE $t/log2 || false
grep -q LC_UUID $t/log2
! grep -q 'uuid 00000000-0000-0000-0000-000000000000' $t/log2 || false

# The default follows ld-prime: arm64 output is ad-hoc signed, x86-64
# output is not (Intel Macs and Rosetta run unsigned code).
$CC --ld-path=$mold -B. -o $t/exe3 $t/a.o
otool -l $t/exe3 > $t/log3
if [ $ARCH = arm64 ]; then
  grep -q LC_CODE_SIGNATURE $t/log3
  codesign -v $t/exe3
else
  not grep -q LC_CODE_SIGNATURE $t/log3
fi
$t/exe3 | grep -F 'Hello world'
