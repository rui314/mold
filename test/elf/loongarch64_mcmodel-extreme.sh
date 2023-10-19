#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -mcmodel=extreme
#include <stdio.h>
char msg[] = "Hello world\n";
int main() { printf(msg); }
EOF

$CC -B. -o $t/exe1 $t/a.o
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-Ttext=0x100000000,-Tdata=0x500000000
$QEMU $t/exe2 | grep -q 'Hello world'
