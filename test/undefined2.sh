#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl _start
_start:
EOF

cat <<EOF | $CC -o $t/b.o -c -x assembler -
.globl ndefined
ndefined:
EOF

cat <<EOF | $CC -o $t/c.o -c -x assembler -
.globl foo
foo:
EOF

rm -f $t/d.a
ar cr $t/d.a $t/b.o $t/c.o

./mold -static -o $t/exe $t/a.o $t/d.a -undefined foo
readelf --symbols $t/exe > $t/log
grep -q foo $t/log
not grep -q ndefined $t/log
