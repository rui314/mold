#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'VER1 { extern "C++" {}; foo; }; VER2 {};' > "$t"/a.ver

cat <<EOF > "$t"/b.s
.globl foo, bar, baz
foo:
  nop
bar:
  nop
baz:
  nop
EOF

$CC -B. -shared -o "$t"/c.so -Wl,-version-script,"$t"/a.ver "$t"/b.s
readelf --dyn-syms "$t"/c.so > "$t"/log
grep -q ' foo@@VER1$' "$t"/log

echo OK
