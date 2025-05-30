#!/bin/bash
. $(dirname $0)/common.inc

seq 1 100000 | sed 's/.*/.globl foo\0\n.section .foo\0,"aw"\nfoo\0:.word 0\n/g' |
  $CC -c -xassembler -o $t/a.o -

cat <<'EOF' | $CC -c -xc -o $t/b.o - -fPIC
extern int foo100000;
int bar() { return foo100000; }
EOF

not $CC -B. -shared -o $t/c.so $t/a.o $t/b.o |& grep -F 'too many output sections'
