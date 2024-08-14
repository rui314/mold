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

cat <<EOF | $CC -fPIC -c -o $t/d.o -xc -
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

cp $t/a.so $t/c.so
$CC -B. -o $t/exe1 $t/d.o $t/c.so -pie || skip
$QEMU $t/exe1 | grep -q 'foo=0x3' || skip
cp $t/b.so $t/c.so
$QEMU $t/exe1 | grep -q 'foo=0x5'

cp $t/a.so $t/c.so
$CC -B. -o $t/exe2 $t/d.o $t/c.so -no-pie
$QEMU $t/exe2 | grep -q 'foo=0x3'
cp $t/b.so $t/c.so
$QEMU $t/exe1 | grep -q 'foo=0x5'
