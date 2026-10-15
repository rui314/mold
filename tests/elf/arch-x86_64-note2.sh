#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Binutils 2.32 injects their own .note.gnu.property section interfering with the tests
test_cflags -Xassembler -mx86-used-note=no && CFLAGS="-Xassembler -mx86-used-note=no" || CFLAGS=""

cat <<EOF | $CC $CFLAGS -o $t/a.o -c -x assembler -
.section .note.a, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | $CC $CFLAGS -o $t/b.o -c -x assembler -
.section .note.b, "a", @note
.p2align 2
.quad 42
EOF

cat <<EOF | $CC $CFLAGS -o $t/c.o -c -x assembler -
.section .note.c, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | $CC $CFLAGS -o $t/d.o -c -xc -
int main() {}
EOF

./mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o

readelf --segments $t/exe | grep -F '01     .note.a .note.c .note.b'

# The segment holds 4-byte aligned .note.b, so it must be 4-byte aligned
# even though .note.a and .note.c are 8-byte aligned. Tools pick the note
# format by a PT_NOTE's p_align.
[ "$(readelf -W --segments $t/exe | awk '$1 == "NOTE" {print $NF}')" = 0x4 ]

# A segment of 8-byte aligned notes alone stays 8-byte aligned.
./mold -o $t/exe2 $t/a.o $t/c.o
[ "$(readelf -W --segments $t/exe2 | awk '$1 == "NOTE" {print $NF}')" = 0x8 ]
