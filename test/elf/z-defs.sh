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
void foo();
void bar() { foo(); }
EOF

$CC -B. -shared -o $t/b.so $t/a.o
$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,nodefs

! $CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,defs \
  2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

! $CC -B. -shared -o $t/b.so $t/a.o -Wl,-no-undefined \
  2> $t/log || false
grep -q 'undefined symbol:.* foo' $t/log

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-z,defs \
  -Wl,--warn-unresolved-symbols 2> $t/log
grep -q 'undefined symbol: .* foo$' $t/log
readelf --dyn-syms $t/c.so | grep -q ' foo$'

echo OK
