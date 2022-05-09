#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

[ "$CC" = cc ] || { echo skipped; exit; }

# ASAN doesn't work with LD_PRELOAD
ldd mold-wrapper.so | grep -q libasan && { echo skipped; exit; }

which clang >& /dev/null || { echo skipped; exit; }

cat <<'EOF' | $CC -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

clang -fuse-ld=bfd -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log || false

LD_PRELOAD=`pwd`/mold-wrapper.so MOLD_PATH=`pwd`/mold \
  clang -o $t/exe $t/a.o -fuse-ld=/usr/bin/ld
readelf -p .comment $t/exe > $t/log
grep -q mold $t/log

echo OK
