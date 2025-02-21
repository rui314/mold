#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC $flags -
#include <stdio.h>
__attribute__((section(".foo"))) void fn1() { printf(" foo"); }
__attribute__((section(".bar"))) void fn2() { printf(" bar"); }

int main(){
  printf("main");
  fn2();
}
EOF

./mold -static -o $t/exe $t/a.o --discard-section .foo -no-pie
$QEMU $t/exe1 | grep 'main bar'
readelf -S $t/exe1 | grep -E '.foo'


./mold -static -o $t/exe $t/a.o --discard-section .foo --no-discard-section -no-pie
$QEMU $t/exe1 | grep 'main bar'
readelf -S $t/exe1 | grep '.foo'
