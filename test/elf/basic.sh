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

echo '.globl _start; _start: jmp loop' | $CC -o $t/a.o -c -x assembler -
echo '.globl loop; loop: jmp loop' | $CC -o $t/b.o -c -x assembler -
"$mold" -static -o $t/exe $t/a.o $t/b.o
objdump -d $t/exe > /dev/null
file $t/exe | grep -q ELF

echo OK
