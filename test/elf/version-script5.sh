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

cat <<EOF > $t/a.ver
{
  extern "C" { foo };
  local: *;
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
int foo = 5;
int main() { return 0; }
EOF

$CC -B. -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.o

readelf --dyn-syms $t/c.so > $t/log
fgrep -q foo $t/log
! fgrep -q ' main' $t/log || false

echo OK
