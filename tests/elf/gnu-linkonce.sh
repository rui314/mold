#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Old GCCs put vague linkage definitions in .gnu.linkonce.* sections,
# which belong to the standard output sections.
cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
__attribute__((section(".gnu.linkonce.t.foo"))) int foo(void) { return 3; }
__attribute__((section(".gnu.linkonce.d.bar"))) int bar = 4;
int main() { printf("%d %d\n", foo(), bar); }
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep '^3 4$'
readelf -SW $t/exe | not grep -F .gnu.linkonce
