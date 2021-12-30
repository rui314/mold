#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<'EOF' > "$t"/a.ver
VER_X1 { *; };
EOF

cat <<EOF | c++ -fPIC -c -o "$t"/b.o -xc -
extern int foo;
int bar() { return foo; }
EOF

clang -fuse-ld="$mold" -shared -Wl,--version-script="$t"/a.ver -o "$t"/c.so "$t"/b.o

readelf --dyn-syms "$t"/c.so > "$t"/log
grep -q 'foo$' "$t"/log
grep -q 'bar@@VER_X1' "$t"/log

echo OK
