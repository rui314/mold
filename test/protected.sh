#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo() __attribute__((visibility("protected")));
int bar() __attribute__((visibility("protected")));
void *baz() __attribute__((visibility("protected")));

int foo() {
  return 4;
}

int bar() {
  return foo();
}

void *baz() {
  return baz;
}
EOF

$CC -B. -o $t/b.so -shared $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc - -fno-PIE
#include <stdio.h>

int foo() {
  return 3;
}

int x = 5;
int bar();
void *baz() { return &x; }

int main() {
  printf("%d %d %d\n", foo(), bar(), baz == baz());
}
EOF

$CC -B. -no-pie -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe 2> /dev/null | grep '3 4 0'
