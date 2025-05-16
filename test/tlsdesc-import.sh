#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc - $tlsdesc_opt
#include <stdio.h>

extern _Thread_local int foo;
extern _Thread_local int bar;

int main() {
  bar = 7;
  printf("%d %d\n", foo, bar);
}
EOF

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc - $tlsdesc_opt
_Thread_local int foo = 5;
_Thread_local int bar;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
$QEMU $t/exe | grep '5 7'
