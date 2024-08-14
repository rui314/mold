#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
#include <stdio.h>
__attribute__((weak)) int foo();
int main() {
  printf("%d\n", foo ? foo() : -1);
}
EOF

cat <<EOF | $CC -c -o $t/b.o -fPIC -xc -
#include <stdio.h>
int foo() { return 2; }
EOF

$CC -B. -o $t/libfoobar.so $t/b.o -shared
$CC -B. -o $t/exe $t/a.o -Wl,--as-needed -L$t -lfoobar -Wl,-rpath,$t

readelf --dynamic $t/exe | grep -q 'NEEDED.*libfoobar'
$QEMU $t/exe | grep -q '^2$'
