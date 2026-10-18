#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Calls between output sections get thunks until the layout shows that
# they don't need them. The removed thunks leave no null .symtab entries.

# MIPS uses JAL, which may need a thunk, only in non-PIC code.
cflags= ldflags=
[[ $MACHINE = mips* ]] && cflags='-fno-pic -mno-abicalls -G0' ldflags=-no-pie

cat <<EOF | $CC $cflags -c -o $t/a.o -xc -
#include <stdio.h>
__attribute__((section(".low"))) void fn1() { printf("Hello"); }
__attribute__((section(".high"))) void fn2() { fn1(); printf(" world\n"); }
int main() { fn2(); }
EOF

$CC -B. $ldflags -o $t/exe $t/a.o
$QEMU $t/exe | grep 'Hello world'

readelf -W --symbols $t/exe | \
  awk '/^Symbol table .\.symtab/ { s = 1 } s && $4 == "NOTYPE" && $7 == "UND" && NF == 7' | \
  wc -l | grep '^1$'
