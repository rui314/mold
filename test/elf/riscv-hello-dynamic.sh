#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ "$(uname -m)" = riscv64 ] && { echo skipped; exit; }

echo 'int main() {}' | riscv64-linux-gnu-gcc -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | riscv64-linux-gnu-gcc -o $t/a.o -c -g -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

riscv64-linux-gnu-gcc -B. -o $t/exe $t/a.o
riscv64-linux-gnu-readelf -p .comment $t/exe | grep -qw mold
riscv64-linux-gnu-readelf -a $t/exe >& $t/log

grep -iqE 'Machine:.*RISC-?V' $t/log
grep -q 'Shared library:.*libc.so' $t/log

qemu-riscv64 -L /usr/riscv64-linux-gnu $t/exe | grep -q 'Hello world'

echo OK
