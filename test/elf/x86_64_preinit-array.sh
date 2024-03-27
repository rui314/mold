#!/bin/bash
. $(dirname $0)/common.inc

is_musl && skip

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl preinit, init, fini

.section .preinit_array,"aw",@preinit_array
.p2align 3
.quad preinit

.section .init_array,"aw",@init_array
.p2align 3
.quad init

.section .fini_array,"aw",@fini_array
.p2align 3
.quad fini
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
#include <unistd.h>

void preinit() { write(STDOUT_FILENO, "preinit ", 8); }
void init() { write(STDOUT_FILENO, "init ", 5); }
void fini() { write(STDOUT_FILENO, "fini\n", 5); }
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'preinit init fini'
