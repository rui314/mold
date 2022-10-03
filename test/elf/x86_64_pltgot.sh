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

cat <<EOF | $CC -fPIC -shared -Wl,-z,noexecstack -o $t/a.so -x assembler -
.globl ext1, ext2
ext1:
  nop
ext2:
  nop
EOF

cat <<EOF | $CC -c -o $t/b.o -x assembler -
.globl _start
_start:
  call ext1@PLT
  call ext2@PLT
  mov ext2@GOTPCREL(%rip), %rax
  ret
EOF

./mold -z separate-loadable-segments -pie -o $t/exe $t/b.o $t/a.so

${TEST_TRIPLE}objdump -d -j .plt.got $t/exe > $t/log

grep -Eq '1034:.*jmp.* <ext2>' $t/log

echo OK
