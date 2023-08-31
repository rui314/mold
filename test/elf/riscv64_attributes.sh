#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -march=rv64imafd_xtheadba1p5 -o $t/a.o -c -xc -
void foo() {}
EOF

# The compiler might not create .riscv.attributes
readelf --sections $t/a.o | grep -Fq .riscv.attributes || skip

cat <<EOF | $CC -march=rv64imafd_xtheadba2p0 -o $t/b.o -c -xc -
void bar() {}
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void _start() {}
EOF

$CC -B. -nostdlib -o $t/exe $t/a.o $t/c.o
readelf -A $t/exe | grep -q '_xtheadba1p5"'

$CC -B. -nostdlib -o $t/exe $t/a.o $t/b.o $t/c.o
readelf -A $t/exe | grep -q '_xtheadba2p0"'
