#!/bin/bash
source "$(dirname "$0")"/common.inc

# Of the labels at a subsection's start, ld-prime's -map names the
# subsection after the best - an exported one before a local, the
# greatest name of equals -, which has the subsection's size; the
# others alias it with none.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.data
.globl _zb, _ab
_zb:
_ab:
_loc:
  .quad 1
.subsections_via_symbols
EOF

# An arm64 assembler labels an empty __text with an ltmpN label at the
# address where the next section, a C string here, starts; the string
# is no less a subsection of its own.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.cstring
L_.a: .asciz "after empty text"
.data
.globl _q
.p2align 3
_q: .quad L_.a
EOF

# A real definition takes a tentative one's place: the symbol is the
# file's that defines it.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.comm _cv,4,2
EOF

cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.globl _cv
_cv: .long 1
.subsections_via_symbols
EOF

# ld-prime credits itself with the alias it makes of a function folded
# into an identical one, which has no size.
cat <<EOF | $CC -o $t/e.o -c -xc -O1 -
__attribute__((noinline)) static int h1(int x) { return x * 7 + 3; }
__attribute__((noinline)) static int h2(int x) { return x * 7 + 3; }
int call(int c) { return h1(c) + h2(c); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o $t/e.o -Wl,-u,_q \
  -Wl,-deduplicate -Wl,-map,$t/map

grep -Eq $'^0x[0-9A-F]+\t0x00000008\t\\[  1\\] _zb$' $t/map
grep -Eq $'^0x[0-9A-F]+\t0x00000000\t\\[  1\\] _ab$' $t/map
grep -Eq $'^0x[0-9A-F]+\t0x00000000\t\\[  1\\] _loc$' $t/map
grep -Eq $'^0x[0-9A-F]+\t0x00000011\t\\[  2\\] literal string: after empty text$' $t/map
grep -Eq $'^0x[0-9A-F]+\t0x00000004\t\\[  4\\] _cv$' $t/map
# (An x86-64 compiler pads h1 to align h2, which makes them differ.)
if [ $ARCH = arm64 ]; then
  grep -Eq $'^0x[0-9A-F]+\t0x00000000\t\\[  0\\] _h2$' $t/map
fi
