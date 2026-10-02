#!/bin/bash
source "$(dirname "$0")"/common.inc

# How -map lists a subsection: a row for each symbol of it, the first
# at a place sized up to the next place or the subsection's end, the
# others there of no size, and an "anon" row for the bytes before the
# first, if any. Labels a compiler or assembler makes for itself (L...,
# l..., an arm64 assembler's ltmpN) name no row.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.data
.p2align 3
.globl _zb, _ab
_zb:
_ab:
_loc:
  .quad 1
.globl _outer, _inner
_outer: .quad 1
.alt_entry _inner
_inner: .quad 2
.section __TEXT,__const
.p2align 3
.quad 3
_c1: .quad 4
l_table: .quad 1, 2
.literal8
.p2align 3
L8: .quad 7
k1: .quad 42
.cstring
str1: .asciz "abc"
.subsections_via_symbols
EOF

# Without .subsections_via_symbols, a section is one subsection, which
# its symbols name part by part.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
.quad 0
.globl _da, _db
_da:
dz:
  .quad 4
_db:
  .quad 5
EOF

# A common symbol is a subsection of the linker's.
echo '.comm _cm,8,3' | $CC -o $t/c.o -c -xassembler -

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -Wl,-map,$t/map
sed -n '/^# Symbols:/,$p' $t/map | grep '^0x' > $t/syms
addr() { printf '0x%X' 0x$(nm $t/exe | awk -v s=$1 '$3 == s { print $1 }'); }
row() { grep -qx "$1"$'\t'"$2"$'\t'"\\[  $3\\] $4" $t/syms; }

# Labels at one place: one has the size.
grep -E '\] (_zb|_ab|_loc)$' $t/syms > $t/one
[ "$(wc -l < $t/one)" -eq 3 ]
[ "$(cut -f1 $t/one | sort -u)" = $(addr _zb) ]
[ "$(cut -f2 $t/one | sort | tr '\n' ' ')" = '0x00000000 0x00000000 0x00000008 ' ]

# An alternate entry point splits its subsection's rows.
row $(addr _outer) 0x00000008 1 _outer
row $(addr _inner) 0x00000008 1 _inner

# The unnamed start of a subsection, and a subsection only a private
# label names, are anon; a literal's label names it.
c1=$(addr _c1)
row $(printf '0x%X' $((c1 - 8))) 0x00000008 1 anon
row $c1 0x00000008 1 _c1
row $(printf '0x%X' $((c1 + 8))) 0x00000010 1 anon
row $(addr k1) 0x00000008 1 k1
row $(addr str1) 0x00000004 1 str1
not grep -q 'l_table\|L8\|ltmp\|_main.*anon' $t/syms

# An object without subsections.
da=$(addr _da)
row $(printf '0x%X' $((da - 8))) 0x00000008 2 anon
grep -E '\] (_da|dz)$' $t/syms > $t/two
[ "$(cut -f1 $t/two | sort -u)" = $da ]
[ "$(cut -f2 $t/two | sort | tr '\n' ' ')" = '0x00000000 0x00000008 ' ]
row $(addr _db) 0x00000008 2 _db

row $(addr _cm) 0x00000008 0 _cm

# A function folded into an identical one (-deduplicate) is listed at
# the copy it folded into, after the copy's own names, of no size. (An
# x86-64 compiler pads h1 to align h2, which makes them differ.)
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/e.o -c -xc -O1 -
__attribute__((noinline)) static int h1(int x) { return x * 7 + 3; }
__attribute__((noinline)) static int h2(int x) { return x * 7 + 3; }
int main(int argc, char **argv) { return h1(argc) + h2(argc) != 20; }
EOF
  $CC --ld-path=$mold -o $t/exe2 $t/e.o -Wl,-deduplicate -Wl,-map,$t/map2
  $t/exe2
  grep -E '\] _h[12]$' $t/map2 > $t/folded
  [ "$(cut -f1 $t/folded | sort -u | wc -l)" -eq 1 ]
  [ "$(sed -n 2p $t/folded | cut -f2-)" = $'0x00000000\t[  1] _h2' ]
fi
