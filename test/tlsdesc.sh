#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

cat <<EOF | $GCC -fPIC -c -o $t/a.o -xc - $tlsdesc_opt
extern _Thread_local int foo;
_Thread_local int bar = 3;

int get_foo() {
  return foo;
}

static _Thread_local int baz = 5;

int get_baz() {
  return baz;
}
EOF

cat <<EOF | $GCC -fPIC -c -o $t/b.o -xc - $tlsdesc_opt
#include <stdio.h>

_Thread_local int foo;
extern _Thread_local int bar;

int get_foo();
int get_baz();

int main() {
  foo = 42;
  printf("%d %d %d\n", get_foo(), bar, get_baz());
  return 0;
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
$QEMU $t/exe1 | grep '42 3 5'

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,-no-relax
$QEMU $t/exe2 | grep '42 3 5'

$CC -B. -shared -o $t/c.so $t/a.o
$CC -B. -o $t/exe3 $t/b.o $t/c.so
$QEMU $t/exe3 | grep '42 3 5'

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-no-relax
$CC -B. -o $t/exe4 $t/b.o $t/c.so -Wl,-no-relax
$QEMU $t/exe4 | grep '42 3 5'
