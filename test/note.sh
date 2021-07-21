#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
.text
.globl _start
_start:
  nop

.section .note.foo, "a", @note
.align 8
.quad 42

.section .note.bar, "a", @note
.align 4
.quad 42

.section .note.baz, "a", @note
.align 8
.quad 42

.section .note.nonalloc, "", @note
.align 1
.quad 42
EOF

$mold -static -o $t/exe $t/a.o
readelf -W --sections $t/exe > $t/log

grep -Pq '.note.bar\s+NOTE.+000008 00   A  0   0  4' $t/log
grep -Pq '.note.baz\s+NOTE.+000008 00   A  0   0  8' $t/log
grep -Pq '.note.nonalloc\s+NOTE.+000008 00      0   0  1' $t/log

readelf --segments $t/exe > $t/log
fgrep -q '01     .note.bar' $t/log
fgrep -q '02     .note.baz .note.foo' $t/log

echo OK
