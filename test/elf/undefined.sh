#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
.globl _start
_start:
EOF

cat <<EOF | cc -o $t/b.o -c -x assembler -
.globl foo
foo:
EOF

cat <<EOF | cc -o $t/c.o -c -x assembler -
.globl bar
bar:
EOF

rm -f $t/d.a
ar cr $t/d.a $t/b.o $t/c.o

$mold -static -o $t/exe $t/a.o $t/d.a
readelf --symbols $t/exe > $t/log
! grep -q foo $t/log || false
! grep -q bar $t/log || false

$mold -static -o $t/exe $t/a.o $t/d.a -u foo
readelf --symbols $t/exe > $t/log
grep -q foo $t/log
! grep -q bar $t/log || false

$mold -static -o $t/exe $t/a.o $t/d.a -u foo --undefined=bar
readelf --symbols $t/exe > $t/log
grep -q foo $t/log
grep -q bar $t/log

echo OK
