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
$t/exe

readelf --dyn-syms $t/b.so > $t/log
fgrep -q 'foo@@ver1' $t/log
fgrep -q 'bar@@ver2' $t/log
! fgrep -q 'baz' $t/log || false

echo OK
