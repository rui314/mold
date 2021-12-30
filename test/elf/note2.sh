#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -x assembler -
.section .note.a, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | cc -o "$t"/b.o -c -x assembler -
.section .note.b, "a", @note
.p2align 2
.quad 42
EOF

cat <<EOF | cc -o "$t"/c.o -c -x assembler -
.section .note.c, "a", @note
.p2align 3
.quad 42
EOF

cat <<EOF | cc -o "$t"/d.o -c -xc -
int main() {}
EOF

"$mold" -static -o "$t"/exe "$t"/a.o "$t"/b.o "$t"/c.o "$t"/d.o

readelf --segments "$t"/exe > "$t"/log
fgrep -q '01     .note.b' "$t"/log
fgrep -q '02     .note.a .note.c' "$t"/log

echo OK
