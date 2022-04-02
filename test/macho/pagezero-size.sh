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
mold="$(pwd)/ld64.mold"
t=out/test/macho/$testname
mkdir -p $t

[ "`uname -p`" = arm ] && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o

otool -l $t/exe | grep -A5 'segname __PAGEZERO' | \
  grep -q 'vmsize 0x0000000100000000'

clang -fuse-ld="$mold" -o $t/exe $t/a.o -Wl,-pagezero_size,0x10000
$QEMU $t/exe | grep -q 'Hello world'

otool -l $t/exe | grep -A5 'segname __PAGEZERO' | \
  grep -q 'vmsize 0x0000000000010000'

echo OK
