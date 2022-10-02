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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl __x86.get_pc_thunk.bx
.section .gnu.linkonce.t.__x86.get_pc_thunk.bx,"ax"
__x86.get_pc_thunk.bx:
  call printf@PLT
EOF

cat <<EOF | $CC -o $t/b.o -c -x assembler -
.globl __x86.get_pc_thunk.bx
.section .text.__x86.get_pc_thunk.bx,"axG",@progbits,foobar,comdat
__x86.get_pc_thunk.bx:
  call puts@PLT
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
${TEST_TRIPLE}objdump -d $t/exe | grep -A1 '<__x86.get_pc_thunk.bx>:' | \
  grep -Fq 'puts@plt'

echo OK
