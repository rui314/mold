#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section .foobar,"aw"
.ascii "foo\0"
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section .foobar,"a"
.ascii "bar\0"
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section .foobar,"ax"
.ascii "bar\0"
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o
readelf -W --sections $t/exe | grep 'foobar.*WAX'
