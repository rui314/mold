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

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/libfoo.so $t/a.o
ar crs $t/libfoo.a $t/a.o

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo();
int main() {
  foo();
}
EOF

$CC -B. -o $t/exe $t/b.o -Wl,--as-needed \
  $t/libfoo.so $t/libfoo.a
ldd $t/exe | grep -q libfoo

$CC -B. -o $t/exe $t/b.o -Wl,--as-needed \
  $t/libfoo.a $t/libfoo.so
! ldd $t/exe | grep -q libfoo || false

echo OK
