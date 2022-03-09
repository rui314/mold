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

"$mold" -o $t/exe $t/a.o
readelf --symbols $t/exe > $t/log
fgrep -q _start $t/log
fgrep -q foo $t/log
fgrep -q .Lbar $t/log

"$mold" -o $t/exe $t/a.o --discard-locals
readelf --symbols $t/exe > $t/log
fgrep -q _start $t/log
fgrep -q foo $t/log
! fgrep -q .Lbar $t/log || false

"$mold" -o $t/exe $t/a.o --discard-all
readelf --symbols $t/exe > $t/log
fgrep -q _start $t/log
! fgrep -q foo $t/log || false
! fgrep -q .Lbar $t/log || false

"$mold" -o $t/exe $t/a.o --strip-all
readelf --symbols $t/exe > $t/log
! fgrep -q _start $t/log || false
! fgrep -q foo $t/log || false
! fgrep -q .Lbar $t/log || false

echo OK
