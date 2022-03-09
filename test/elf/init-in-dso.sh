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

cat <<EOF | $CC -shared -o $t/a.so -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo();
int main() {}
EOF

$CC -B. -o $t/exe $t/a.so $t/b.o -Wl,-init,foo
readelf --dynamic $t/exe > $t/log
! fgrep -q '(INIT)' $t/log || false

echo OK
