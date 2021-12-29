#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

# Skip if target is not x86-64
[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | clang -fcf-protection=branch -c -o "$t"/a.o -xc -
void _start() {}
EOF

cat <<EOF | clang -fcf-protection=none -c -o "$t"/b.o -xc -
void _start() {}
EOF

"$mold" -o "$t"/exe "$t"/a.o
readelf -n "$t"/exe | grep -q 'x86 feature: IBT'

"$mold" -o "$t"/exe "$t"/b.o
! readelf -n "$t"/exe | grep -q 'x86 feature: IBT' || false

echo OK
