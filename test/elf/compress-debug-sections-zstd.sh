#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

# arm-linux-gnueabihf-objcopy crashes on x86-64
[[ $MACHINE = arm* ]] && { echo skipped; exit; }

command -v zstdcat >& /dev/null || { echo skipped; exit; }

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

echo OK
