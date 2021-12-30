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
void foo(int x) {}
void bar(int x) {}
EOF

cat <<EOF | c++ -o "$t"/b.o -c -xc++ -
void baz(int x) {}
int main() {}
EOF

clang++ -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o

readelf --dyn-syms "$t"/exe > "$t"/log
! grep -q ' foo$' "$t"/log || false
! grep -q ' bar$' "$t"/log || false

cat <<EOF > "$t"/dyn
{ foo; extern "C++" { "baz(int)"; }; };
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -Wl,-dynamic-list="$t"/dyn

readelf --dyn-syms "$t"/exe > "$t"/log
grep -q ' foo$' "$t"/log
! grep -q ' bar$' "$t"/log || false
grep -q ' _Z3bazi$' "$t"/log

echo OK
