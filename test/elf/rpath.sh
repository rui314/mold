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
  .globl main
main:
  nop
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o \
  -Wl,-rpath,/foo -Wl,-rpath,/bar -Wl,-R/no/such/directory -Wl,-R/

readelf --dynamic "$t"/exe | \
  fgrep -q 'Library runpath: [/foo:/bar:/no/such/directory:/]'

echo OK
