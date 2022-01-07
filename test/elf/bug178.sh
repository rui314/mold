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

# Verify that mold does not crash if no object file is included
# in the output. The resulting executable doesn't contain any
# meaningful code or data, so this is an edge case, though.

cat <<EOF | $CC -x assembler -c -o "$t"/a.o -
.globl foo
foo:
EOF

rm -f "$t"/a.a
ar rcs "$t"/a.a "$t"/a.o

"$mold" -o "$t"/exe "$t"/a.a

echo OK
