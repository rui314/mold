#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -shared -fPIC -o $t/a.so -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -shared -fPIC -o $t/b.so -xc -
int bar() { return 3; }
EOF

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
int foo();
int baz() { return foo(); }
EOF

$CC -shared -o $t/c.so $t/c.o $t/a.so

cat <<EOF | $CC -c -o $t/d.o -xc -
#include <stdio.h>
int baz();
int main() {
  printf("%d\n", baz());
}
EOF

$CC -B. -o $t/exe $t/d.o -Wl,--as-needed $t/c.so $t/b.so $t/a.so
$QEMU $t/exe | grep -q '^3$'

readelf --dynamic $t/exe > $t/log
! grep -q /a.so $t/log || false
grep -q /c.so $t/log || false
! grep -q /b.so $t/log || false
