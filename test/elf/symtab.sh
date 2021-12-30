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
  .globl foo, bar, this_is_global
local1:
foo:
bar:
  .byte 0
EOF

cat <<EOF | cc -o "$t"/b.o -c -x assembler -
  .globl this_is_global
local2:
this_is_global:

  .globl module_local
module_local:
EOF

echo '{ local: module_local; };' > "$t"/c.map

"$mold" -o "$t"/exe "$t"/a.o "$t"/b.o --version-script="$t"/c.map

readelf --symbols "$t"/exe > "$t"/log

grep -Pq '0 NOTYPE  LOCAL  DEFAULT    \d+ local1' "$t"/log
grep -Pq '0 NOTYPE  LOCAL  DEFAULT    \d+ local2' "$t"/log
grep -Pq '0 NOTYPE  GLOBAL DEFAULT    \d+ foo' "$t"/log
grep -Pq '0 NOTYPE  GLOBAL DEFAULT    \d+ bar' "$t"/log
grep -Pq '0 NOTYPE  GLOBAL DEFAULT    \d+ this_is_global' "$t"/log
grep -Pq '0 NOTYPE  GLOBAL DEFAULT    \d+ module_local' "$t"/log

echo OK
