#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -o "$t"/a.o -xc -
int foo();
int main() {
  foo();
}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o 2>&1 \
  | grep -q 'undefined symbol:.*foo'

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o --warn-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o --warn-unresolved-symbols \
  --error-unresolved-symbols 2>&1 \
  | grep -q 'undefined symbol:.*foo'

echo OK
