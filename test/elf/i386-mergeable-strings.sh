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
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

echo 'int main() {}' | $CC -m32 -o /dev/null -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<'EOF' | $CC -o $t/a.o -c -x assembler -m32 -
  .text
  .globl main
main:
  push $.L.str+3
  call printf
  add $4, %esp
  push $.rodata.str1.1+16
  call printf
  add $4, %esp
  xor %eax, %eax
  ret

  .section .rodata.str1.1, "aMS", @progbits, 1
  .string "bar"
.L.str:
  .string "xyzHello"
  .string "foo world\n"
EOF

$CC -B. -m32 -static -o $t/exe $t/a.o
$QEMU $t/exe | grep -q 'Hello world'

echo OK
