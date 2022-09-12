#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF > $t/a.ver
{
  global:
  extern "C++" {
    foo::bar;
  };

  local: *;
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc++ -
int bar = 5;
namespace foo {
int bar = 7;
}

int main() {
  return 0;
}
EOF

$CC -B. -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep -Fq _ZN3foo3barE $t/log
! grep -Fq ' bar' $t/log || false

echo OK
