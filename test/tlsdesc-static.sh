#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip
test_cflags -static || skip

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc - $tlsdesc_opt
#include <stdio.h>

extern _Thread_local int foo;

int main() {
  foo = 42;
  printf("%d\n", foo);
}
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc - $tlsdesc_opt
_Thread_local int foo;
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -static
$QEMU $t/exe1 | grep 42

$CC -B. -o $t/exe2 $t/a.o $t/b.o -static -Wl,-no-relax
$QEMU $t/exe2 | grep 42
