#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The arm64 kernel requires the standard 4 GiB pagezero for any
# process, so a shrunken pagezero only runs on Intel hosts.
[ "$(uname -m)" = arm64 ] && skip

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
otool -l $t/exe | grep -A5 'segname __PAGEZERO' | grep -q 'vmsize 0x0000000100000000'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-pagezero_size,0x10000
$t/exe2 | grep 'Hello world'
otool -l $t/exe2 | grep -A5 'segname __PAGEZERO' | grep -q 'vmsize 0x0000000000010000'
