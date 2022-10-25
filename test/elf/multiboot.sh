#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
__attribute__((section("multiboot"), aligned(32)))
const char hdr[32] = {0};
void _entry() {}
EOF

./mold -o $t/exe1 $t/a.o --image-base=0x100000
readelf -SW $t/exe1 | grep -q 'multiboot.*0100040'

./mold -o $t/exe2 $t/a.o --image-base=0x100000 -gc-sections
readelf -SW $t/exe2 | grep -q 'multiboot.*0100040'

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
__attribute__((section("multiboot")))
const char hdr[32] = {0};
void _entry() {}
EOF

./mold -o $t/exe3 $t/b.o --image-base=0x100000 2>&1 | \
  grep -q 'multiboot section alignment'
