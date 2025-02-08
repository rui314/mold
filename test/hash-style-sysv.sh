#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,--hash-style=sysv

cat <<EOF | $CC -o $t/c.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so -Wl,--hash-style=sysv
$QEMU $t/exe | grep Hello
