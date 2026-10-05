#!/usr/bin/env bash
. $(dirname $0)/common.inc

# ADR and LDR (literal) referring to a symbol in another section. GNU as
# cannot assemble them, so use clang.
cat <<'EOF' > $t/a.s
.syntax unified
.arch armv7-a
.text
.globl thm_adr, thm_ldr, arm_adr, arm_ldr
.type thm_adr, %function
.type thm_ldr, %function
.type arm_adr, %function
.type arm_ldr, %function

.thumb
.thumb_func
thm_adr:
  adr.w r0, foo
  adr.w r1, bar
  ldr r1, [r1]
  ldr r0, [r0]
  add r0, r1
  bx lr

.thumb_func
thm_ldr:
  ldr.w r0, foo
  ldr.w r1, bar
  add r0, r1
  bx lr

.arm
arm_adr:
  adr r0, foo
  adr r1, bar
  ldr r1, [r1]
  ldr r0, [r0]
  add r0, r1
  bx lr

arm_ldr:
  ldr r0, foo
  ldr r1, bar
  add r0, r1
  bx lr

.section .text.foo, "ax"
.globl foo
foo:
  .word 30
EOF

cat <<'EOF' > $t/b.s
.text
.globl bar
bar:
  .word 12
EOF

clang ${TRIPLE:+--target=$TRIPLE} -c -o $t/a.o $t/a.s
clang ${TRIPLE:+--target=$TRIPLE} -c -o $t/b.o $t/b.s

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>
int thm_adr(), thm_ldr(), arm_adr(), arm_ldr();
int main() { printf("%d %d %d %d\n", thm_adr(), thm_ldr(), arm_adr(), arm_ldr()); }
EOF

$CC -B. -o $t/exe1 $t/b.o $t/a.o $t/c.o
$QEMU $t/exe1 | grep '^42 42 42 42$'

./mold -r -o $t/d.o $t/b.o $t/a.o
$CC -B. -o $t/exe2 $t/d.o $t/c.o
$QEMU $t/exe2 | grep '^42 42 42 42$'
