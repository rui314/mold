#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>

extern _Thread_local int foo;
static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

$CC -fPIC -ftls-model=local-dynamic -c -o $t/b.o $t/a.c
$CC -fPIC -ftls-model=local-dynamic -c -o $t/c.o $t/a.c -O2

cat <<EOF | $CC -fPIC -ftls-model=local-dynamic -c -o $t/d.o -xc -
_Thread_local int foo = 3;
EOF

$CC -B. -o $t/exe1 $t/b.o $t/d.o -Wl,-relax
$QEMU $t/exe1 | grep '3 5 3 5'

$CC -B. -o $t/exe2 $t/c.o $t/d.o -Wl,-relax
$QEMU $t/exe2 | grep '3 5 3 5'

$CC -B. -o $t/exe3 $t/b.o $t/d.o -Wl,-no-relax
$QEMU $t/exe3 | grep '3 5 3 5'

$CC -B. -o $t/exe4 $t/c.o $t/d.o -Wl,-no-relax
$QEMU $t/exe4 | grep '3 5 3 5'
