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

cat <<EOF | $CC -shared -fPIC -o $t/a.so -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -shared -fPIC -o $t/b.so -xc -
int bar() { return 3; }
EOF

cat <<EOF | $CC -shared -fPIC -o $t/c.so -xc -
int foo();
int baz() { return foo(); }
EOF

cat <<EOF | $CC -c -o $t/d.o -xc -
#include <stdio.h>
int baz();
int main() {
  printf("%d\n", baz());
}
EOF

$CC -B. -o $t/exe $t/d.o -Wl,--as-needed \
  $t/c.so $t/b.so $t/a.so

readelf --dynamic $t/exe > $t/log
grep -q /a.so $t/log
grep -q /c.so $t/log
! grep -q /b.so $t/log || false

echo OK
