#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe1 $t/a.o
$CC -B. -o $t/exe2 $t/a.o -Wl,--no-mmap-output-file
cmp $t/exe1 $t/exe2
$QEMU $t/exe2 | grep 'Hello world'
