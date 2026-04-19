#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/libfoobar.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void hello();
int main() { hello(); }
EOF

$CC -B. -o $t/exe1 $t/c.o -L$t -Wl,--library,foobar -Wl,-rpath,$t
$QEMU $t/exe1 | grep 'Hello world'

$CC -B. -o $t/exe2 $t/c.o -L$t -Wl,--library=foobar -Wl,-rpath,$t
$QEMU $t/exe2 | grep 'Hello world'
