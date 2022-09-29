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

cat <<EOF > $t/a.ver
ver1 {
  global: ?oo;
  local: *;
};

ver2 {
  global: b?r;
};
EOF

cat <<EOF | $CC -B. -xc -shared -o $t/b.so -Wl,-version-script,$t/a.ver -
void foo() {}
void bar() {}
void baz() {}
EOF

cat <<EOF | $CC -xc -c -o $t/c.o -
void foo();
void bar();

int main() {
  foo();
  bar();
  return 0;
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe

readelf --dyn-syms $t/b.so > $t/log
grep -Fq 'foo@@ver1' $t/log
grep -Fq 'bar@@ver2' $t/log
! grep -Fq 'baz' $t/log || false

echo OK
