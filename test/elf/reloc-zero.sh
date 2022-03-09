#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit 0; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
foo: jmp 0
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$CC -B. -no-pie -o $t/exe $t/a.o $t/b.o

echo OK
