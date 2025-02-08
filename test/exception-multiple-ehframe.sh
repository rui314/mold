#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = sh4aeb ] && skip
nm mold | grep '__tsan_init' && skip
command -v perl > /dev/null || skip

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

perl -i -0777 -pe 's/\.eh_frame/.EH_FRAME/g' $t/a.o
./mold -r -o $t/c.o $t/a.o $t/b.o
perl -i -0777 -pe 's/\.EH_FRAME/.eh_frame/g' $t/c.o

cat <<EOF | $CXX -o $t/d.o -c -xc++ -
#include <stdio.h>

int foo();
int bar();

int main() {
  printf("%d %d\n", foo(), bar());
}
EOF

$CXX -B. -o $t/exe1 $t/d.o $t/c.o
$QEMU $t/exe1 | grep '^1 3$'
