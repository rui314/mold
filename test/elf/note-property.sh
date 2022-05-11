#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

# Skip if target is not x86-64
[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -fcf-protection=branch -c -o $t/a.o -xc -
void _start() {}
EOF

cat <<EOF | $CC -fcf-protection=none -c -o $t/b.o -xc -
void _start() {}
EOF

./mold -o $t/exe $t/a.o
readelf -n $t/exe | grep -q 'x86 feature: IBT'

./mold -o $t/exe $t/b.o
! readelf -n $t/exe | grep -q 'x86 feature: IBT' || false

echo OK
