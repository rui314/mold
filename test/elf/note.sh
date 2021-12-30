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
.text
.globl _start
_start:
  nop

.section .note.foo, "a", @note
.p2align 3
.quad 42

.section .note.bar, "a", @note
.p2align 2
.quad 42

.section .note.baz, "a", @note
.p2align 3
.quad 42

.section .note.nonalloc, "", @note
.p2align 0
.quad 42
EOF

"$mold" -static -o "$t"/exe "$t"/a.o
readelf -W --sections "$t"/exe > "$t"/log

grep -Pq '.note.bar\s+NOTE.+000008 00   A  0   0  4' "$t"/log
grep -Pq '.note.baz\s+NOTE.+000008 00   A  0   0  8' "$t"/log
grep -Pq '.note.nonalloc\s+NOTE.+000008 00      0   0  1' "$t"/log

readelf --segments "$t"/exe > "$t"/log
fgrep -q '01     .note.bar' "$t"/log
fgrep -q '02     .note.baz .note.foo' "$t"/log
! grep -q 'NOTE.*0x0000000000000000 0x0000000000000000' "$t"/log || false

echo OK
