#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
int foo = 3;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC -O
extern int foo;
int get_foo() { return foo; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
#include <stdio.h>
int get_foo();
int main() { printf("%d\n", get_foo()); }
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -pie -Wl,--no-relax
$QEMU $t/exe1 | grep -q '^3$'
$OBJDUMP -d $t/exe1 | grep -A2 '<get_foo>:' | grep -Fqw pcalau12i
$OBJDUMP -d $t/exe1 | grep -A2 '<get_foo>:' | grep -Fqw ld.d

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -pie -Wl,--relax
$QEMU $t/exe2 | grep -q '^3$'
$OBJDUMP -d $t/exe2 | grep -A1 '<get_foo>:' | grep -Fqw pcaddi

$CC -B. -o $t/exe3 $t/a.o $t/b.o $t/c.o -pie -Wl,--relax \
  -Wl,-Ttext=0x1000000,-Tdata=0x2000000

$QEMU $t/exe3 | grep -q '^3$'
$OBJDUMP -d $t/exe3 | grep -A2 '<get_foo>:' | grep -Fqw pcalau12i
$OBJDUMP -d $t/exe3 | grep -A2 '<get_foo>:' | grep -Fqw addi.d
