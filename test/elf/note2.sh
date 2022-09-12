#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

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

./mold -static -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o

readelf --segments $t/exe > $t/log
grep -Fq '01     .note.a .note.c .note.b' $t/log

echo OK
