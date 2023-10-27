#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -mcmodel=extreme
#include <stdio.h>
char msg[] = "Hello world\n";
int main() { printf(msg); }
EOF

$CC -B. -o $t/exe1 $t/a.o
$QEMU $t/exe1 | grep -q 'Hello world'
