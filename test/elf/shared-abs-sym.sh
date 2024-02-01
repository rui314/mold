#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xassembler -
.globl foo
foo = 3;
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF > $t/c.c
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

$CC -fPIC -c -o $t/d.o $t/c.c
$CC -B. -o $t/exe1 -pie $t/d.o $t/b.so
$QEMU $t/exe1 | grep -q 'foo=0x3'

$CC -B. -o $t/exe2 -no-pie $t/d.o $t/b.so
$QEMU $t/exe2 | grep -q 'foo=0x3'
