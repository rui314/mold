#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = arm ] && skip

cat <<EOF | $CXX -o $t/a.o -c -fPIC -xc++ -
#include <iostream>
int main() { std::cout << "Hello world\n"; }
EOF

$CXX -B. -o $t/exe $t/a.o -Wl,-emit-relocs
$QEMU $t/exe | grep -q 'Hello world'

readelf -SW $t/exe | grep -Eq 'rela?\.text'
readelf -SW $t/exe | grep -Eq 'rela?\.eh_frame'
