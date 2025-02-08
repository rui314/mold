#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl foo
.weak bar
foo:
  la a0, bar
  ret
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
long foo();
int main() { printf("%ld\n", foo()); }
EOF

$CC -B. -static -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^0$'
