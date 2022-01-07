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

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-default-symver
readelf --dyn-syms $t/b.so | grep -q ' foo@@b\.so$'

$CC -B. -o $t/b.so -shared $t/a.o \
  -Wl,--soname=bar -Wl,-default-symver
readelf --dyn-syms $t/b.so | grep -q ' foo@@bar$'

echo OK
