#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -B. -fPIC -shared -o $t/a.so -xassembler -
.globl foo
foo = 3;
EOF

cat <<EOF | $CC -B. -fPIC -shared -o $t/b.so -xassembler -
.globl foo
foo = 5;
EOF

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

# This test fails with older glibc
$CC -B. -o $t/exe1 -pie $t/c.o $t/a.so 2> /dev/null || skip
$QEMU $t/exe1 | grep -q 'foo=0x3' || skip
LD_PRELOAD=$t/b.so $QEMU $t/exe1 | grep -q 'foo=0x5'

$CC -B. -o $t/exe2 -pie $t/c.o $t/a.so
$QEMU $t/exe2 | grep -q 'foo=0x3'
LD_PRELOAD=$t/b.so $QEMU $t/exe2 | grep -q 'foo=0x5'

$CC -B. -o $t/exe3 -no-pie $t/c.o $t/a.so
$QEMU $t/exe3 | grep -q 'foo=0x3'
LD_PRELOAD=$t/b.so $QEMU $t/exe3 | grep -q 'foo=0x5'
