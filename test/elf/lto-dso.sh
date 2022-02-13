#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -flto -c -fPIC -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so -flto $t/a.o
nm -D $t/b.so | grep -q 'T foo'

echo OK
