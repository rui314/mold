#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
__attribute__((section("multiboot"), aligned(32)))
const char hdr[32] = {0};
void _entry() {}
EOF

./mold -o $t/exe $t/a.o --image-base=0x100000
readelf -SW $t/exe | grep -q 'multiboot.*0100040'
