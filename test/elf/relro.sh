#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -c -xc -o $t/a.o -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_RELRO ' $t/log

$CC -B. -o $t/exe $t/a.o -Wl,-z,norelro
readelf --segments -W $t/exe > $t/log
! grep -q 'GNU_RELRO ' $t/log || false

echo OK
