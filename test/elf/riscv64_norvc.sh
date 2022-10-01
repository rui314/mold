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

[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] || { echo skipped; exit; }

# Disable C extension
if [ $MACHINE = riscv32 ]; then
    ISA=rv32g
else
    ISA=rv64g
fi

cat <<EOF | $CC -march=$ISA -O2 -o $t/a.o -c -xc -
int add1(int n) { return n + 1; }
EOF

cat <<EOF | $CC -march=$ISA -O2 -o $t/b.o -c -xc -
int add1(int n);
int add2(int n) { n += 1; return add1(n); }
EOF

cat <<EOF | $CC -march=$ISA -O2 -o $t/c.o -c -xc -
int add2(int n);
int main() {
  add2(0);
  return 0;
}
EOF

$CC -march=$ISA -B. -nostdlib -O2 -o $t/exe $t/a.o $t/b.o $t/c.o

${TEST_TRIPLE}objdump -d $t/exe | grep -q ff5ff06f # j pc - 0xc

echo OK
