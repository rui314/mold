#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = i386 ] && skip
[ $MACHINE = m68k ] && skip
[ $MACHINE = arm ] && skip
[ $MACHINE = ppc64 ] && skip
[ $MACHINE = ppc64le ] && skip
[ $MACHINE = sh4 ] && skip
[ $MACHINE = alpha ] && skip

cat <<EOF | $CC -shared -o $t/a.so -xc -
int foo = 3;
int bar = 5;
EOF

cat <<EOF | $CC -fno-PIC -c -o $t/b.o -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d\n", foo, bar);
  return 0;
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.so $t/b.o
$QEMU $t/exe | grep -q '3 5'

! $CC -B. -o $t/exe $t/a.so $t/b.o -no-pie -Wl,-z,nocopyreloc 2> $t/log || false

grep -q 'recompile with -fPIC' $t/log
