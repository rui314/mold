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
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

"$mold" -e foo -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q 'Entry point address:.*0x201000' "$t"/log

"$mold" -e bar -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q 'Entry point address:.*0x201008' "$t"/log

"$mold" -static -o "$t"/exe "$t"/a.o
readelf -e "$t"/exe > "$t"/log
grep -q 'Entry point address:.*0x201000' "$t"/log

echo OK
