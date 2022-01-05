#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

# Skip if target is not x86-64
[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | cc -o "$t"/a.o -c -xc -
void _start() {}
EOF

"$mold" -o "$t"/exe "$t"/a.o
readelf --file-header "$t"/exe | grep -qi x86-64

echo OK
