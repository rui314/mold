#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>

void fn3();
void fn4();

__attribute__((section(".low"))) void fn1() { printf(" fn1"); fn3(); }
__attribute__((section(".low"))) void fn2() { printf(" fn2"); fn4(); }

int main() {
  printf(" main");
  fn1();
  printf("\n");
}
EOF

cat <<EOF > $t/b.c
#include <stdio.h>

void fn1();
void fn2();

__attribute__((section(".high"))) void fn3() { printf(" fn3"); fn2(); }
__attribute__((section(".high"))) void fn4() { printf(" fn4"); }
EOF

$CC -c -o $t/c.o $t/a.c
$CC -c -o $t/d.o $t/b.c

$CC -B. -o $t/exe1 $t/c.o $t/d.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x100000000
$QEMU $t/exe1 | grep 'main fn1 fn3 fn2 fn4'

$CC -B. -o $t/exe2 $t/c.o $t/d.o \
  -Wl,--section-start=.high=0x10000000,--section-start=.low=0x100000000
$QEMU $t/exe2 | grep 'main fn1 fn3 fn2 fn4'


$CC -c -o $t/e.o $t/a.c -fno-PIC -mcmodel=large
$CC -c -o $t/f.o $t/b.c -fno-PIC -mcmodel=large

$CC -B. -o $t/exe3 $t/e.o $t/f.o -pie \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x400000000
$QEMU $t/exe3 | grep 'main fn1 fn3 fn2 fn4'

$CC -B. -o $t/exe4 $t/e.o $t/f.o -pie \
  -Wl,--section-start=.high=0x10000000,--section-start=.low=0x400000000
$QEMU $t/exe4 | grep 'main fn1 fn3 fn2 fn4'
