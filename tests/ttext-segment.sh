#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIE -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

# GNU ld's -Ttext-segment spelling sets the first load segment address.
$CC -B. -no-pie -o $t/exe1 $t/a.o -Wl,-Ttext-segment=0x9000000
$QEMU $t/exe1 | grep 'Hello world'
readelf -W --segments $t/exe1 | grep -E 'LOAD\s+0x0+\s+0x0*9000000\b'

# -Ttext-segment turns a PIE into a fixed-address ET_EXEC while retaining
# DF_1_PIE. Test the double-dash, separate-argument spelling too.
$CC -B. -pie -o $t/exe2 $t/a.o -Wl,--Ttext-segment,0xa000000
$QEMU $t/exe2 | grep 'Hello world'
readelf -W --file-header $t/exe2 | grep -E 'Type:\s+EXEC'
readelf -W --dynamic $t/exe2 | grep -E 'FLAGS_1.*PIE'
readelf -W --segments $t/exe2 | grep -E 'LOAD\s+0x0+\s+0x0*a000000\b'

# Test that zero is recognized as an explicitly specified address.
$CC -B. -pie -o $t/exe3 $t/a.o -Wl,-Ttext-segment=0
readelf -W --file-header $t/exe3 | grep -E 'Type:\s+EXEC'
