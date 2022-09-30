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

cat <<'EOF' | $CC -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  sub $8, %rsp
  lea msg(%rip), %rdi
  xor %rax, %rax
  call printf@PLT
  xor %rax, %rax
  add $8, %rsp
  ret

  .data
msg:
  .string "Hello world\n"
EOF

$CC -B. -o $t/exe $t/a.o

readelf --sections $t/exe | grep -Fq '.got'
readelf --sections $t/exe | grep -Fq '.got.plt'

$QEMU $t/exe | grep -q 'Hello world'

echo OK
