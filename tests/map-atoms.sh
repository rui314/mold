#!/bin/bash
source "$(dirname "$0")"/common.inc

# How ld-prime's -map names atoms. A linker-private label (l...) names
# its atom, but not in a section ld-prime reads as records (selector
# references here), whose atoms are "anon" like any no symbol names; a
# fixed-size literal no symbol names is known by its size, and every C
# string by its contents, named or not.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__const
.p2align 3
l_table: .quad 1, 2
.literal8
.p2align 3
L8: .quad 7
.cstring
.globl _named_str
_named_str: .asciz "named"
.section __TEXT,__objc_methname,cstring_literals
l_meth: .asciz "doIt"
.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip
.p2align 3
l_selref: .quad l_meth
.subsections_via_symbols
EOF

# Without .subsections_via_symbols, symbols still split sections into
# atoms, and of symbols at one place the first in the symbol table has
# the size while the others alias it with none: on arm64, that is the
# ltmpN label the assembler puts at the start of each section.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
.globl _first, _second
_first:
_second:
  .quad 1
EOF

# An alternate entry point, such as Swift's type metadata inside its
# full metadata, aliases a place inside another symbol's atom.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.p2align 3
.globl _outer, _inner
_outer: .quad 1
.alt_entry _inner
_inner: .quad 2
.subsections_via_symbols
EOF

# ld64 names no literal after a linker-private label: a fixed-size
# literal the compiler labels lCPI0_0 is known by its size all the same.
# With subsections, an arm64 section's ltmpN label names the atom at the
# section's start only if nothing else does (here the first of __const).
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.quad 3
_c1: .quad 4
.literal16
.p2align 4
lCPI0_0: .quad 9, 10
.subsections_via_symbols
EOF

$mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$(xcrun --show-sdk-path)" \
  -o $t/exe $t/a.o $t/b.o $t/d.o $t/e.o -lSystem -map $t/map
sym() { grep -F $'\t'"[  $1] $2" $t/map | cut -f2; }
[ "$(sym 1 l_table)" = 0x00000010 ]
[ "$(sym 1 8-byte-literal)" = 0x00000008 ]
[ "$(sym 1 'literal string: named')" = 0x00000006 ]
[ "$(sym 1 'literal string: doIt')" = 0x00000005 ]
[ "$(sym 1 anon)" = 0x00000008 ]
[ "$(sym 2 _second)" = 0x00000000 ]
if [ $ARCH = arm64 ]; then
  [ "$(sym 2 ltmp1)" = 0x00000008 ]
  [ "$(sym 2 _first)" = 0x00000000 ]
else
  [ "$(sym 2 _first)" = 0x00000008 ]
fi
[ "$(sym 3 _outer)" = 0x00000010 ]
[ "$(sym 3 _inner)" = 0x00000000 ]
not grep -q 'l_selref\|l_meth\|\[  1\] ltmp\|_named_str\|lCPI0_0' $t/map
[ "$(sym 4 16-byte-literal)" = 0x00000010 ]
[ "$(sym 4 _c1)" = 0x00000008 ]
if [ $ARCH = arm64 ]; then
  [ "$(sym 4 ltmp1)" = 0x00000008 ]
else
  [ "$(sym 4 anon)" = 0x00000008 ]
fi

# With lazy binding, a stub's lazy pointer counts as the defining file
# too, and the stub helper's entries are anonymous; __dyld_private and
# the binder's GOT slot come with them. The re-exported libraries that
# define the symbols follow the named files by install name.
cat <<EOF | $CC -o $t/c.o -c -xc - -mmacosx-version-min=11.0
#include <stdio.h>
int main() { puts("hi"); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/c.o -mmacosx-version-min=11.0 -Wl,-map,$t/map2
grep -Eq '^\[  3\] .*/system/libdyld.tbd$' $t/map2
grep -Eq '^\[  4\] .*/system/libsystem_c.tbd$' $t/map2
grep -Fq $'\t[  4] _puts.lazy_ptr' $t/map2
grep -Fq $'\t[  3] dyld_stub_binder.got' $t/map2
grep -Fq $'\t[  0] __dyld_private' $t/map2
[ "$(grep -Fc $'\t[  0] anon' $t/map2)" = 2 ]

# A static executable's -stack_size stack shows as ld-prime models it:
# a zero-fill __UNIXSTACK,__stack section holding an l__unixstack atom.
$mold -arch $ARCH -static -stack_size 0x8000 -e _main $t/a.o -o $t/exe3 -map $t/map3
grep -Eq $'^0x[0-9A-F]+\t0x00008000\t__UNIXSTACK\t__stack$' $t/map3
grep -Fq $'\t0x00008000\t[  0] l__unixstack' $t/map3
