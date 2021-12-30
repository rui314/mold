#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -o "$t"/a.o -xc++ -
int foo(int, int);
int main() {
  foo(3, 4);
}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-no-demangle 2> "$t"/log || false
grep -q 'undefined symbol: .*: _Z3fooii' "$t"/log

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-demangle 2> "$t"/log || false
grep -Pq 'undefined symbol: .*: foo\(int, int\)' "$t"/log

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o 2> "$t"/log || false
grep -Pq 'undefined symbol: .*: foo\(int, int\)' "$t"/log

cat <<EOF | clang -c -o "$t"/b.o -xc -
extern int Pi;
int main() {
  return Pi;
}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/b.o -Wl,-demangle 2> "$t"/log || false
grep -q 'undefined symbol: .*: Pi' "$t"/log

echo OK
