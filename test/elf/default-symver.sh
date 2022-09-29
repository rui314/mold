#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-default-symver
readelf --dyn-syms $t/b.so | grep -q ' foo@@b\.so$'

$CC -B. -o $t/b.so -shared $t/a.o \
  -Wl,--soname=bar -Wl,-default-symver
readelf --dyn-syms $t/b.so | grep -q ' foo@@bar$'

echo OK
