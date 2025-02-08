#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = arm ] && skip

cat <<EOF | $CXX -o $t/a.o -c -fPIC -xc++ -
#include <cstdio>
int main() { printf("Hello world\n"); }
EOF

$CXX -B. -o $t/exe $t/a.o -Wl,-emit-relocs
$QEMU $t/exe | grep 'Hello world'

readelf -SW $t/exe | grep -E 'rela?\.text'
readelf -SW $t/exe | grep -E 'rela?\.eh_frame'
