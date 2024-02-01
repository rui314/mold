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

# This test fails with older glibc
$CC -o $t/exe1 -pie $t/d.o $t/b.so 2> /dev/null || skip
$QEMU $t/exe1 | grep -q 'foo=0x3' || skip

$CC -B. -o $t/exe2 -pie $t/d.o $t/b.so
$QEMU $t/exe2 | grep -q 'foo=0x3'

$CC -B. -o $t/exe3 -no-pie $t/d.o $t/b.so
$QEMU $t/exe3 | grep -q 'foo=0x3'
