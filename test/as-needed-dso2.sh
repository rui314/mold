#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
int foo() {
  return 0;
}
EOF

cat <<EOF | $CC -c -fPIC -o $t/b.o -xc -
__attribute__((weak)) int foo();

int bar() {
  if (foo) return foo();
  return 0;
}
EOF

cat <<EOF | $CC -xc -c -o $t/c.o -
int bar();

int main() {
  return bar();
}
EOF

$CC -B. -shared -o $t/libfoo.so $t/a.o
$CC -B. -shared -o $t/libbar.so $t/b.o
$CC -B. -o $t/exe $t/c.o -L$t -Wl,--as-needed -lfoo -lbar

readelf --dynamic $t/exe > $t/log
! grep libfoo.so $t/log || false
grep -q libbar.so $t/log
