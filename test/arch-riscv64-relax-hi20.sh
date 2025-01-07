#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl get_foo, get_foo2, get_bar, get_bar2, get_baz
get_foo:
  lui a0, %hi(foo)
  add a0, a0, %lo(foo)
  ret
get_foo2:
  lui a0, %hi(foo+0x10000000)
  add a0, a0, %lo(foo)
  ret
get_bar:
  lui a0, %hi(bar)
  add a0, a0, %lo(bar)
  ret
get_bar2:
  lui a0, %hi(bar+0x1ffff)
  add a0, a0, %lo(bar+0x1ffff)
  ret
get_baz:
  lui a0, %hi(baz)
  add a0, a0, %lo(baz)
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl foo, bar, baz
foo = 0xf00
bar = 0
baz = 0x11beef
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

int get_foo();
int get_foo2();
int get_bar();
int get_bar2();
int get_baz();

int main() {
  printf("%x %x %x %x %x\n",
         get_foo(), get_foo2(), get_bar(), get_bar2(), get_baz());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--no-relax
$QEMU $t/exe1 | grep -q 'f00 10000f00 0 1ffff 11beef'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe2 | grep -q 'f00 10000f00 0 1ffff 11beef'

[ $(stat --format='%s' $t/exe1) -gt $(stat --format='%s' $t/exe2) ]
