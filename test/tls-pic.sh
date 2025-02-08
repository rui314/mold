#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $GCC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((tls_model("global-dynamic"))) extern _Thread_local int foo;
__attribute__((tls_model("global-dynamic"))) static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | $CC -xc -c -o $t/b.o -
__attribute__((tls_model("global-dynamic"))) _Thread_local int foo = 3;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '3 5 3 5'
