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

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

echo 'int main() {}' | aarch64-linux-gnu-gcc -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | aarch64-linux-gnu-gcc -o $t/a.o -c -g -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

aarch64-linux-gnu-gcc -B. -o $t/exe $t/a.o -static

readelf -p .comment $t/exe | grep -qw mold

readelf -a $t/exe > $t/log
grep -Eq 'Machine:\s+AArch64' $t/log
qemu-aarch64 -L /usr/aarch64-linux-gnu $t/exe | grep -q 'Hello world'

echo OK
