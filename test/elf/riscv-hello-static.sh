#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

echo 'int main() {}' | riscv64-linux-gnu-gcc-10 -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | riscv64-linux-gnu-gcc-10 -o $t/a.o -c -g -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

# riscv64-linux-gnu-gcc-10 -B"`dirname "$mold"`" -o $t/exe $t/a.o -static
riscv64-linux-gnu-gcc-10 -B. -o $t/exe $t/a.o -static

# riscv64-linux-gnu-readelf -p .comment $t/exe | grep -qw mold

riscv64-linux-gnu-readelf -a $t/exe > $t/log
# grep -Eq 'Machine:\s+Riscv64' $t/log
qemu-riscv64 -L /usr/riscv64-linux-gnu $t/exe | grep -q 'Hello world'

echo OK
