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

# IFUNC is not supported on RISC-V yet
[ "$(uname -m)" = riscv64 ] && { echo skipped; exit; }

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | $CC -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

# -static-pie works only with a newer version of glibc
ldd --version 2>&1 | grep -Pq 'Copyright \(C\) 202[2-9]' || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -static-pie
$t/exe | grep -q 'Hello world'

echo OK
