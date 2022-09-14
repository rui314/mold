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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

echo 'int main() {}' | $CC -o /dev/null -xc - -static >& /dev/null || \
  { echo skipped; exit; }

echo '.globl _start; _start: jmp loop' | $CC -o $t/a.o -c -x assembler -
echo '.globl loop; loop: jmp loop' | $CC -o $t/b.o -c -x assembler -
./mold -static -o $t/exe $t/a.o $t/b.o
$OBJDUMP -d $t/exe > /dev/null
file $t/exe | grep -q ELF

echo OK
