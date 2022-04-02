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

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo1() {}
void foo2() {}

__asm__(".symver foo1, bar1@");
__asm__(".symver foo2, bar2@@");
EOF

$CC -B. -shared -o $t/b.so $t/a.o

readelf --dyn-syms $t/b.so | grep -q 'bar1$'
readelf --dyn-syms $t/b.so | grep -q 'bar2$'

echo OK
