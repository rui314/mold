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
int foo();

int main() {
  foo();
}
EOF

! "$mold" -o "$t"/exe "$t"/a.o 2> "$t"/log || false
grep -q 'undefined symbol: .*\.o: foo' "$t"/log

echo OK
