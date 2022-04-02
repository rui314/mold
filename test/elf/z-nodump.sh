#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o
! readelf --dynamic $t/b.so | grep -Eq 'Flags:.*NODUMP' || false

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,nodump
readelf --dynamic $t/b.so | grep -Eq 'Flags:.*NODUMP'

echo OK
