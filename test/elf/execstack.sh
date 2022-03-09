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

cat <<EOF | $CC -c -xc -o $t/a.o -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RWE ' $t/log

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack \
  -Wl,-z,noexecstack
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RW ' $t/log

$CC -B. -o $t/exe $t/a.o
readelf --segments -W $t/exe > $t/log
grep -q 'GNU_STACK.* RW ' $t/log

echo OK
