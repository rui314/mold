#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.section .note.a, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | $CC -o $t/b.o -c -x assembler -
.section .note.b, "a", @note
.p2align 2
.quad 42
EOF

cat <<EOF | $CC -o $t/c.o -c -x assembler -
.section .note.c, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
int main() {}
EOF

./mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o

readelf --segments $t/exe > $t/log
grep -Fq '01     .note.a .note.c .note.b' $t/log
