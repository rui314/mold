#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.syntax unified
.globl bar
.thumb
bar:
 beq foo-2
EOF

$OBJDUMP -d $t/a.o | grep -E 'beq\.n' || skip

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl foo, baz
.thumb
baz:
 nop
foo:
 nop
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void bar();
int main() { bar(); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
$OBJDUMP -d $t/exe | grep -E 'beq\.n.*<baz>'
