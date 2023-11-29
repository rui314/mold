#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = m68k ] && skip
[ $MACHINE = sh4 ] && skip

cat <<EOF | $CXX -o $t/a.o -c -xc++ -
int foo() {
  try {
    throw 1;
  } catch (int x) {
    return x;
  }
  return 2;
}
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
int bar() {
  try {
    throw 3;
  } catch (int x) {
    return x;
  }
  return 4;
}
EOF

$OBJCOPY --rename-section .eh_frame=.eh_frame2 $t/a.o
./mold -r -o $t/c.o $t/a.o $t/b.o
$OBJCOPY --rename-section .eh_frame2=.eh_frame $t/c.o

cat <<EOF | $CXX -o $t/d.o -c -xc++ -
#include <stdio.h>

int foo();
int bar();

int main() {
  printf("%d %d\n", foo(), bar());
}
EOF

$CXX -B. -o $t/exe1 $t/d.o $t/c.o
$QEMU $t/exe1
$QEMU $t/exe1 | grep -q '^1 3$'
