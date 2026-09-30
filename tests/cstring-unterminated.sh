#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C-string section may end in a string without its NUL. ld-prime
# takes it for a string all the same, completes it with a NUL in a
# final image and a -r output alike, and merges it with no other
# string, not even an identical one.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
.asciz "xy"
.globl _tail
_tail: .ascii "abc"
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__cstring,cstring_literals
L_str: .asciz "abc"
.data
.globl _ptr
.p2align 3
_ptr: .quad L_str
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern const char tail[];
extern const char *ptr;
int main() { printf("%s %s %d\n", tail, ptr, tail == ptr); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/main.o
$t/exe > $t/out
grep -qx 'abc abc 0' $t/out

# "xy", "abc" and "abc", each with its NUL.
$mold -arch $ARCH -r -o $t/r.o $t/a.o $t/b.o
size=$(otool -l $t/r.o | awk '$1 == "sectname" { n = $2 } n == "__cstring" && $1 == "size" { print $2 }')
[ $((size)) = 11 ]
$CC --ld-path=$mold -o $t/exe2 $t/r.o $t/main.o
$t/exe2 > $t/out2
grep -qx 'abc abc 0' $t/out2
