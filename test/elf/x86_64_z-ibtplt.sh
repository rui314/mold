#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello"); }
void world() { printf("world"); }
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-z,ibtplt

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

void hello();
void world();

int main() {
  hello();
  printf(" ");
  world();
  printf("\n");
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so -Wl,-z,ibtplt
$QEMU $t/exe | grep -q 'Hello world'
