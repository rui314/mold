#!/bin/bash
. $(dirname $0)/common.inc

# arm-linux-gnueabihf-objcopy crashes on x86-64
[ $MACHINE = arm ] && skip

command -v zstdcat >& /dev/null || skip

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zstd
${TEST_TRIPLE}objcopy --dump-section .debug_info=$t/debug_info $t/exe
dd if=$t/debug_info of=$t/debug_info.zstd bs=24 skip=1 status=none
zstdcat $t/debug_info.zstd > /dev/null
