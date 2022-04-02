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

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-init,foo
readelf --dynamic $t/exe | fgrep -q '(INIT)'

$CC -B. -o $t/exe $t/a.o -Wl,-init,no-such-symbol
readelf --dynamic $t/exe > $t/log
! fgrep -q '(INIT)' $t/log || false

echo OK
