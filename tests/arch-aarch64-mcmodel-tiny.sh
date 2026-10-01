#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -mcmodel=tiny -c -o $t/a.o -xc -
extern int foo;
int *get_addr() { return &foo; }
int get_foo() { return foo; }
EOF

$OBJDUMP -r $t/a.o | grep R_AARCH64_GOT_LD_PREL19 || skip

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc -
int foo = 42;
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>
extern int foo;
int *get_addr();
int get_foo();
int main() { printf("%d %d\n", get_foo(), get_addr() == &foo); }
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.so $t/c.o
$QEMU $t/exe1 | grep '^42 1$'

cat <<EOF | $CC -c -o $t/d.o -xc -
int foo = 42;
EOF

$CC -B. -o $t/exe2 $t/a.o $t/c.o $t/d.o
$QEMU $t/exe2 | grep '^42 1$'
