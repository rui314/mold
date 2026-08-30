#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The assembler emits (alignment - minimum instruction size) bytes of NOPs
# for an alignment directive, so the addend of R_RISCV_ALIGN is itself a
# power of two for `.balign 4` in RVC code and for `.balign 8` in non-RVC
# code. Make sure that the linker infers the right alignment from them.

cat <<EOF | $CC -o $t/a.o -c -xassembler - -march=rv64gc
.globl x1
.text
.p2align 4
  nop
.balign 4
x1:
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler - -march=rv64g
.globl x2
.text
.p2align 4
  nop
.balign 8
x2:
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
#include <stdint.h>
extern char x1, x2;
int main() {
  printf("%lu %lu\n", (uintptr_t)&x1 % 4, (uintptr_t)&x2 % 8);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep '^0 0$'
