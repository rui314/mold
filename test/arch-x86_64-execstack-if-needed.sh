#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xassembler -o $t/a.o -
.globl main
main:
  ret
.section .note.GNU-stack, "x", @progbits
EOF

$CC -B. -o $t/exe $t/a.o >& /dev/null
readelf --segments -W $t/exe | grep 'GNU_STACK.* RW '

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack-if-needed
readelf --segments -W $t/exe | grep 'GNU_STACK.* RWE '
