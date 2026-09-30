#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C string keeps its input offset modulo its section's alignment, as
# any atom does, and of identical strings ld-prime keeps the copy that
# alignment favors most (an atom at 16 mod 32 is 16-aligned), the first
# of equals. Swift pads the strings of its 16-aligned __objc_methname
# and __objc_classname with NULs so that many start at a multiple of
# 16; its copy of a name then wins over clang's unaligned one, whose
# object comes first here.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__objc_classname,cstring_literals
L5: .asciz "q"
L6: .asciz "0123456789abcdefghij"
L7: .asciz "xy"
.data
.p2align 3
.globl _p2
_p2: .quad L5, L6, L7
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__objc_classname,cstring_literals
.p2align 4
L1: .asciz "abc"
.byte 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
L2: .asciz "0123456789abcdefghij"
.byte 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
L3: .asciz "xy"
L4: .asciz "zzz"
.data
.p2align 3
.globl _p1
_p1: .quad L1, L2, L3, L4
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern const char *p1[4], *p2[3];
int main() {
  printf("%d %d %d ", p1[1] == p2[1], p1[2] == p2[2], p1[0] == p2[0]);
  for (int i = 0; i < 4; i++)
    printf("%s:%d ", p1[i], (int)((unsigned long)p1[i] % 16));
  printf("\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/b.o
$t/exe | grep -q '^1 1 0 abc:0 0123456789abcdefghij:0 xy:0 zzz:3 $'
otool -l $t/exe | awk '$1 == "sectname" && $2 == "__objc_classname" { f = 1 }
  f && $1 == "size" { print $2; f = 0 }' > $t/size
grep -q '^0x0000000000000047$' $t/size
