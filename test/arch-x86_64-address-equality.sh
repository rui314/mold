#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl foo
foo:
  lea bar(%rip), %rax
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
#include <stdio.h>
void *foo();
void bar();
int main() { printf("%d %p %p\n", foo() == bar, foo(), bar); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
void bar() {}
EOF

$CC -B. -shared -o $t/d.so $t/c.o

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/d.so -pie
$QEMU $t/exe1 | grep '^1 '

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/d.so -pie -Wl,-no-relax
$QEMU $t/exe2 | grep '^1 '
