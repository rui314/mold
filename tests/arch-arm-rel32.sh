#!/usr/bin/env bash
. $(dirname $0)/common.inc

# R_ARM_REL32 against a symbol defined in a DSO can only be resolved by
# copying the symbol into the executable (or, for a function, by giving
# it a canonical PLT entry), like any other PC-relative relocation.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .globl get_foo, get_bar
  .type get_foo, %function
  .type get_bar, %function
get_foo:
  ldr r0, 1f
2:
  add r0, pc, r0
  ldr r0, [r0]
  bx lr
1: .word foo - (2b + 8)
get_bar:
  ldr r0, 1f
2:
  add r0, pc, r0
  bx lr
1: .word bar - (2b + 8)
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <stdio.h>
int get_foo();
void *get_bar();
void *bar();
int main() { printf("%d %d\n", get_foo(), get_bar() == bar()); }
EOF

cat <<EOF | $CC -fPIC -shared -o $t/c.so -xc -
int foo = 42;
void *bar() { return bar; }
EOF

$CC -B. -pie -o $t/exe1 $t/a.o $t/b.o $t/c.so
$QEMU $t/exe1 | grep '^42 1$'

$CC -B. -no-pie -o $t/exe2 $t/a.o $t/b.o $t/c.so
$QEMU $t/exe2 | grep '^42 1$'

not $CC -B. -shared -o $t/d.so $t/a.o $t/c.so |& grep 'recompile with -fPIC'
