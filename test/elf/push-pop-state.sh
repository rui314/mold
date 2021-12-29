#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | clang -shared -o "$t"/a.so -xc -
int foo = 1;
EOF

cat <<EOF | clang -shared -o "$t"/b.so -xc -
int bar = 1;
EOF

cat <<EOF | clang -c -o "$t"/c.o -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/c.o -Wl,-as-needed \
  -Wl,-push-state -Wl,-no-as-needed "$t"/a.so -Wl,-pop-state "$t"/b.so

readelf --dynamic "$t"/exe > "$t"/log
fgrep -q a.so "$t"/log
! fgrep -q b.so "$t"/log || false

echo OK
