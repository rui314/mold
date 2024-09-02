#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl _start
_start:
EOF

echo 'int fn1() { return 42; }' | $CC -o $t/b.o -c -xc -
echo 'int fn2() { return 42; }' | $CC -o $t/c.o -c -xc -

rm -f $t/d.a
ar cr $t/d.a $t/b.o $t/c.o

$CC -B. -nostdlib -o $t/exe $t/a.o $t/d.a

readelf --symbols $t/exe > $t/log
! grep -q fn1 $t/log || false
! grep -q fn2 $t/log || false

$CC -B. -nostdlib -o $t/exe $t/a.o -Wl,--whole-archive $t/d.a

readelf --symbols $t/exe > $t/log
grep -q fn1 $t/log
grep -q fn2 $t/log

$CC -B. -nostdlib -o $t/exe $t/a.o -Wl,--whole-archive \
  -Wl,--no-whole-archive $t/d.a

readelf --symbols $t/exe > $t/log
! grep -q fn1 $t/log || false
! grep -q fn2 $t/log || false
