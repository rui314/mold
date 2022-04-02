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

echo '.globl _start; _start: jmp loop' | $CC -o $t/a.o -c -x assembler -
echo '.globl loop; loop: jmp loop' | $CC -o $t/b.o -c -x assembler -
echo "-o '$t/exe' '$t/a.o' '$t/b.o'" > $t/rsp
"$mold" -static @$t/rsp
$OBJDUMP -d $t/exe > /dev/null
file $t/exe | grep -q ELF

echo OK
