#!/bin/bash
. $(dirname $0)/common.inc


cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl init1, init2, fini1, fini2

.section .init_array,"aw",@init_array
.p2align 3
.quad init1

.section .init_array,"aw",@init_array
.p2align 3
.quad init2

.section .fini_array,"aw",@fini_array
.p2align 3
.quad fini1

.section .fini_array,"aw",@fini_array
.p2align 3
.quad fini2
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

void init1() { printf("init1 "); }
void init2() { printf("init2 "); }
void fini1() { printf("fini1\n"); }
void fini2() { printf("fini2 "); }

int main() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'init1 init2 fini2 fini1'
