#!/bin/bash
source "$(dirname "$0")"/common.inc

# Only the kept copy of a weak definition asks for GOT slots. Swift's
# symbolic type references are weak: the copy in the object defining a
# type refers to its descriptor directly, every other object's through
# a GOT slot, which a link keeping the direct copy doesn't need.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__const
.p2align 2
.globl _target
_target: .long 42
.globl _ref
.private_extern _ref
.weak_definition _ref
_ref: .long _target - .
.subsections_via_symbols
EOF

if [ $ARCH = arm64 ]; then
  got='_target@GOT - .'
else
  got='_target@GOTPCREL'
fi
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__const
.p2align 2
.long 0
.globl _ref
.private_extern _ref
.weak_definition _ref
_ref: .long $got
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern const int ref;
int main() { printf("%d\n", *(const int *)((const char *)&ref + ref)); }
EOF

got_size() {
  otool -l $1 | awk '$1 == "sectname" && $2 == "__got" { f = 1 }
    f && $1 == "size" { print $2; f = 0 }'
}
$CC --ld-path=$mold -o $t/exe1 $t/main.o $t/a.o
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o
$t/exe2 | grep -q '^42$'
[ "$(got_size $t/exe1)" = "$(got_size $t/exe2)" ]
