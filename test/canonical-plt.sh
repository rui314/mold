#!/bin/bash
. $(dirname $0)/common.inc

# GCC produces buggy code for this test case on s390x.
# https://sourceware.org/bugzilla/show_bug.cgi?id=29655
[ $MACHINE = s390x ] && $CC -v |& grep -E '^gcc version 1[0-5]\.' && skip

cat <<EOF | $CC -o $t/a.so -fPIC -shared -xc -
void *foo() {
  return foo;
}

void *bar() {
  return bar;
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
void *bar();

void *baz() {
  return bar;
}
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fno-PIC
#include <stdio.h>

void *foo();
void *bar();
void *baz();

int main() {
  printf("%d %d %d\n", foo == foo(), bar == bar(), bar == baz());
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.so $t/b.o $t/c.o
$QEMU $t/exe | grep '^1 1 1$'
