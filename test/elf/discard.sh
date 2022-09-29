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

[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -Wa,-L -
  .text
  .globl _start
_start:
  nop
foo:
  nop
.Lbar:
  nop
EOF

./mold -o $t/exe $t/a.o
readelf --symbols $t/exe > $t/log
grep -Fq _start $t/log
grep -Fq foo $t/log
grep -Fq .Lbar $t/log

./mold -o $t/exe $t/a.o --discard-locals
readelf --symbols $t/exe > $t/log
grep -Fq _start $t/log
grep -Fq foo $t/log
! grep -Fq .Lbar $t/log || false

./mold -o $t/exe $t/a.o --discard-all
readelf --symbols $t/exe > $t/log
grep -Fq _start $t/log
! grep -Fq foo $t/log || false
! grep -Fq .Lbar $t/log || false

./mold -o $t/exe $t/a.o --strip-all
readelf --symbols $t/exe > $t/log
! grep -Fq _start $t/log || false
! grep -Fq foo $t/log || false
! grep -Fq .Lbar $t/log || false

echo OK
