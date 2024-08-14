#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

cat <<EOF | $GCC -fPIC -ftls-model=local-dynamic -c -o $t/a.o -xc - $tlsdesc_opt
extern _Thread_local int foo;

int get_foo() {
  return foo;
}

static _Thread_local int bar = 5;

int get_bar() {
  return bar;
}
EOF

cat <<EOF | $GCC -fPIC -ftls-model=local-dynamic -c -o $t/b.o -xc - $tlsdesc_opt
#include <stdio.h>

_Thread_local int foo;

int get_foo();
int get_bar();

int main() {
  foo = 42;
  printf("%d %d\n", get_foo(), get_bar());
  return 0;
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
$QEMU $t/exe1 | grep -q '42 5'

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--no-relax
$QEMU $t/exe2 | grep -q '42 5'
