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

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o
readelf --dynamic $t/exe > $t/log
fgrep -q '(DEBUG)' $t/log

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo() {}
EOF

$CC -B. -o $t/c.so $t/b.o -shared
readelf --dynamic $t/c.so > $t/log
! fgrep -q '(DEBUG)' $t/log || false

echo OK
