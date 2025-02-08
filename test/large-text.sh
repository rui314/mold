#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl foo
foo:
.space 20*1024*1024
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep 'Hello world'
