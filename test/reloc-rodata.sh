#!/bin/bash
. $(dirname $0)/common.inc
exit

[ $MACHINE = aarch64 ] && skip

cat <<EOF | $CC -fno-PIC -c -o $t/a.o -xc -
#include <stdio.h>

int foo;
int * const bar = &foo;

int main() {
  printf("%d\n", *bar);
}
EOF

not $CC -B. -o $t/exe $t/a.o -pie |&
  grep -E 'relocation against symbol .+ can not be used; recompile with -fPIC'
