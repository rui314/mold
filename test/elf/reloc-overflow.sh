#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | cc -o "$t"/a.o -c -x assembler -
  .globl foo
  .data
foo:
  .short foo
EOF

! "$mold" -e foo -static -o "$t"/exe "$t"/a.o 2> "$t"/log || false
fgrep -q 'relocation R_X86_64_16 against foo out of range' "$t"/log

echo OK
