#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
#include <stdio.h>

void foo() {}
int bar() __attribute__((ifunc("resolve_bar")));
void *resolve_bar() { return foo; }
void *bar2 = bar;

int main() {
  printf("%p %p\n", bar, bar2);
}
EOF

$CC -B. -o $t/exe1 $t/a.o -pie
$QEMU $t/exe1 | grep -Eq '^(\S+) \1$'

$CC -B. -o $t/exe2 $t/a.o -no-pie
$QEMU $t/exe2 | grep -Eq '^(\S+) \1$'
