#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -x assembler -c -o $t/a.o -x assembler -

.section .foo,"a"
.ascii "foo\0"
.section .bar,"a"
.ascii "bar\0"
.section .text
.globl _start
_start:
EOF

./mold -o $t/exe0 $t/a.o
readelf -S $t/exe0 | grep '.foo'
readelf -S $t/exe0 | grep '.bar'

./mold -o $t/exe1 $t/a.o --discard-section='.foo'
readelf -S $t/exe1 | grep -v '.foo'
readelf -S $t/exe1 | grep '.bar'

./mold -o $t/exe2 $t/a.o --discard-section='foo' --discard-section='boo' --no-discard-section='foo'
readelf -S $t/exe2 | grep '.foo'
readelf -S $t/exe2 | grep -v '.bar'
