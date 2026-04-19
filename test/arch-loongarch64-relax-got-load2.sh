#!/bin/bash
. $(dirname $0)/common.inc

# pcaddi takes a 20-bit immediate that is shifted left by 2 to form
# a PC-relative offset. Therefore, relaxing a GOT load (pcalau12i +
# ld.d) to pcaddi is valid only when the symbol's address is 4-byte
# aligned. Here, `foo` is placed at an odd address, so the relaxation
# must not happen.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.byte 0
.globl foo
.type foo, @object
foo:
.byte 0x42
.size foo, 1
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC -O
extern char foo;
int get_foo() { return foo; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int get_foo();
int main() { printf("%d\n", get_foo()); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -pie -Wl,--relax
$QEMU $t/exe | grep '^66$'
$OBJDUMP -d $t/exe | grep -A2 '<get_foo>:' | grep -Fw pcalau12i
