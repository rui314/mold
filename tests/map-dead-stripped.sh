#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -dead_strip, ld-prime's -map lists every atom of the input files
# that the output doesn't have, whatever took it out, by file and in
# the order they were in it. A tentative definition (a common symbol)
# is an atom of its file's, of the size it gives, listed after the
# file's others: all are dead but the one whose symbol the output
# defines (here b.o's larger _aa), and a real definition (c.o's _zz)
# takes the place of all.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.comm _zz,4,2
.comm _aa,16,3
.data
_d1: .quad 1
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.comm _aa,32,3
.comm _zz,2,1
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.globl _zz
_zz: .long 7
.subsections_via_symbols
EOF

# Coalescing leaves atoms out too: a C string equal to another file's,
# a weak definition another file's won. ld-prime makes a C string an
# atom per label at its start, merging all but one into that one.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.cstring
l_.a:
l_.b: .asciz "two labels"
l_.c: .asciz "dup"
.data
.p2align 3
.globl _p, _wk
.weak_definition _wk
_p: .quad l_.a
.quad l_.c
_wk: .quad 1
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/e.o -c -xassembler -
.cstring
l_.c: .asciz "dup"
.data
.p2align 3
.globl _p2, _wk
.weak_definition _wk
_p2: .quad l_.c
_wk: .quad 2
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o $t/e.o -Wl,-dead_strip \
  -Wl,-u,_aa -Wl,-u,_p -Wl,-u,_p2 -Wl,-u,_wk -Wl,-map,$t/map

sed -n '/^# Dead Stripped Symbols:/,$p' $t/map | grep -v '^#' > $t/dead
cat > $t/expected <<EOF
<<dead>>	0x00000008	[  1] _d1
<<dead>>	0x00000010	[  1] _aa
<<dead>>	0x00000004	[  1] _zz
<<dead>>	0x00000002	[  2] _zz
<<dead>>	0x00000004	[  3] _zz
<<dead>>	0x0000000B	[  4] literal string: two labels
<<dead>>	0x00000004	[  5] literal string: dup
<<dead>>	0x00000008	[  5] _wk
EOF
diff $t/expected $t/dead

# The labels of a string count once each: the live one is listed too.
grep -Eq $'^0x[0-9A-F]+\t0x0000000B\t\\[  4\\] literal string: two labels$' $t/map
