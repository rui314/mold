#!/bin/bash
. $(dirname $0)/common.inc

# arm-linux-gnueabihf-objcopy crashes on x86-64
[[ $MACHINE = arm* ]] && skip
[ $MACHINE = riscv32 ] && skip

command -v zstdcat >& /dev/null || skip

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

# Test zstd-1 (lowest level)
$CC -B. -o $t/exe1 $t/a.o -Wl,--compress-debug-sections=zstd-1
$OBJCOPY --dump-section .debug_info=$t/debug_info1 $t/exe1
dd if=$t/debug_info1 of=$t/debug_info1.zstd bs=24 skip=1 status=none
zstdcat $t/debug_info1.zstd > /dev/null

# Test zstd-19 (high level)
$CC -B. -o $t/exe19 $t/a.o -Wl,--compress-debug-sections=zstd-19
$OBJCOPY --dump-section .debug_info=$t/debug_info19 $t/exe19
dd if=$t/debug_info19 of=$t/debug_info19.zstd bs=24 skip=1 status=none
zstdcat $t/debug_info19.zstd > /dev/null
