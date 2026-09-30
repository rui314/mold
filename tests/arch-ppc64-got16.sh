#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .section .opd, "aw"
  .align 3
  .globl get_foo
  .type get_foo, @function
get_foo:
  .quad .L.get_foo, .TOC.@tocbase, 0

  .text
.L.get_foo:
  addi 4, 2, foo@got
  ld 3, 0(4)
  blr
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

int foo;
int *get_foo(void);

int main() {
  printf("%d\n", get_foo() == &foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^1$'
