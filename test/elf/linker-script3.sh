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

mkdir -p $t/foo

cat <<EOF | $CC -o $t/foo/a.o -c -xc -
int main() {}
EOF

cat <<EOF > $t/b.script
INPUT(a.o)
EOF

$CC -o $t/exe -L$t/foo $t/b.script

echo OK
