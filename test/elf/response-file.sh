#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

echo '.globl _start; _start: jmp loop' | $CC -o $t/a.o -c -x assembler -
echo '.globl loop; loop: jmp loop' | $CC -o $t/b.o -c -x assembler -
echo "-o '$t/exe' '$t/a.o' '$t/b.o'" > $t/rsp
"$mold" -static @$t/rsp
objdump -d $t/exe > /dev/null
file $t/exe | grep -q ELF

echo OK
