#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
void foo() {}
EOF

clang -fuse-ld="$mold" -o "$t"/b.so -shared "$t"/a.o -Wl,-default-symver
readelf --dyn-syms "$t"/b.so | grep -q ' foo@@b\.so$'

clang -fuse-ld="$mold" -o "$t"/b.so -shared "$t"/a.o \
  -Wl,--soname=bar -Wl,-default-symver
readelf --dyn-syms "$t"/b.so | grep -q ' foo@@bar$'

echo OK
