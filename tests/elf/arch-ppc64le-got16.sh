#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .abiversion 2
  .text
  .globl get_foo
  .type get_foo, @function
get_foo:
0:
  addis 2, 12, .TOC.-0b@ha
  addi 2, 2, .TOC.-0b@l
  .localentry get_foo, .-get_foo

  addi 4, 2, foo@got
  ld 4, 0(4)
  std 4, 0(3)

  addis 4, 2, foo@got@ha
  addi 4, 4, foo@got@l
  ld 4, 0(4)
  std 4, 8(3)

  lis 4, foo@got@h
  ori 4, 4, foo@got@l
  ldx 4, 4, 2
  std 4, 16(3)
  blr
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

int foo;
void get_foo(int **p);

int main() {
  int *p[3];
  get_foo(p);
  printf("%d %d %d\n", p[0] == &foo, p[1] == &foo, p[2] == &foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^1 1 1$'
