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

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
extern int foo;
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
__attribute__((visibility("protected"))) int foo;
EOF

$CC -B. -shared -o $t/c.so $t/a.o $t/b.o -Wl,-strip-all
readelf --symbols $t/c.so | grep -Pq 'PROTECTED\b.*\bfoo\b'

echo OK
