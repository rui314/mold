#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime names an unnamed subsection "anon-N" in its diagnostics, for
# the object's Nth subsection: section by section in section header
# order, by address within one. A label that names no subsection is a
# subsection of its own: a second label at a place (numbered before the
# one with the bytes, which on a literal a private L or l label takes),
# an alternate entry point, any label past a section's start in an
# object without subsections - and on arm64 there, the ltmpN label of a
# literal. The private labels name no literal, which ld-prime merges by
# content. Each __compact_unwind record is a subsection; an empty
# section, an __LLVM one and __eh_frame have none.
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 3
.quad 0
.quad L5
.quad Lc1
.quad Lc2
.globl _f
_f: ret
.cstring
Lc1: .asciz "a"
Lc2: .asciz "b"
.data
.quad 0
L5: .quad 0
EOF

not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> $t/log-a
if [ $ARCH = arm64 ]; then
  grep -q "text-relocation in 'ltmp0'+0x8 (.*/a.o) to 'ltmp2'$" $t/log-a
  grep -q "text-relocation in 'ltmp0'+0x10 (.*/a.o) to 'anon-2'$" $t/log-a
  grep -q "text-relocation in 'ltmp0'+0x18 (.*/a.o) to 'anon-4'$" $t/log-a
else
  grep -q "text-relocation in 'anon-0'+0x8 (.*/a.o) to 'anon-4'$" $t/log-a
  grep -q "text-relocation in 'anon-0'+0x10 (.*/a.o) to 'anon-2'$" $t/log-a
  grep -q "text-relocation in 'anon-0'+0x18 (.*/a.o) to 'anon-3'$" $t/log-a
fi

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _fn
_fn: ret
.section __TEXT,__c1
.quad Lc1
.quad l_c2
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _fn
.long 1
.long 0
.quad 0
.quad 0
.section __LLVM,__foo
.quad 0
.section __TEXT,__c2
.globl _a, _b, _c
.p2align 3
_a:
_b: .quad Lc1
.alt_entry _c
_c: .quad l_c2
.cstring
Lc1: .asciz "a"
.globl _s
_s:
l_c2: .asciz "b"
.subsections_via_symbols
EOF

not $CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o 2> $t/log-b
if [ $ARCH = arm64 ]; then
  grep -q "text-relocation in 'ltmp1' (.*/b.o) to 'anon-6'$" $t/log-b
  grep -q "text-relocation in 'ltmp1'+0x8 (.*/b.o) to 'anon-8'$" $t/log-b
  grep -q "text-relocation in '_b' (.*/b.o) to 'anon-6'$" $t/log-b
  grep -q "text-relocation in '_b'+0x8 (.*/b.o) to 'anon-8'$" $t/log-b
else
  grep -q "text-relocation in 'anon-1' (.*/b.o) to 'anon-6'$" $t/log-b
  grep -q "text-relocation in 'anon-1'+0x8 (.*/b.o) to 'anon-8'$" $t/log-b
  grep -q "text-relocation in '_b' (.*/b.o) to 'anon-6'$" $t/log-b
  grep -q "text-relocation in '_b'+0x8 (.*/b.o) to 'anon-8'$" $t/log-b
fi
