#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.syntax unified
.globl bar
.thumb
bar:
 beq foo+8
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo() {}
void bar();
int main() { bar(); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$OBJDUMP -d $t/exe > $t/log
grep -Eq 'beq\.w.*<foo\+0x8>' $t/log
