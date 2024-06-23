#!/bin/bash
. $(dirname $0)/common.inc

# Looks like lockf doesn't work correctly on qemu-riscv64
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIE
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

for i in `seq 1 20`; do
  rm -f $t/exe$i
  ( MOLD_JOBS=2 $CC -B. -o $t/exe$i $t/a.o -no-pie; echo $i) &
done

wait

for i in `seq 1 20`; do
  $QEMU $t/exe$i | grep -q 'Hello world'
done
