#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

void world() {
  printf("world\n");
}

void real_hello() {
  printf("Hello ");
  world();
}

void hello() {
  real_hello();
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

void world() {
  printf("WORLD\n");
}

void hello();

int main() {
  hello();
}
EOF

$CC -B. -o $t/exe -Wl,-rpath=$t $t/c.o $t/b.so
$QEMU $t/exe | grep 'Hello WORLD'
