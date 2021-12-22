#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -x assembler -
  .text
  .globl _start
_start:
  nop
EOF

"$mold" -o "$t"/b.so "$t"/a.o --filter foo -F bar -shared

readelf --dynamic "$t"/b.so > "$t"/log
fgrep -q 'Filter library: [foo]' "$t"/log
fgrep -q 'Filter library: [bar]' "$t"/log

echo OK
