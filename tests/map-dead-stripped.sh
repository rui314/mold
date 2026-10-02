#!/bin/bash
source "$(dirname "$0")"/common.inc

# Under -dead_strip, -map lists the subsections of the input files that
# dead stripping removed, file by file in the order they were in it,
# named and sized as live ones are. A subsection that gave way to an
# identical one - a C string equal to another file's, a weak definition
# another file's won - is no dead one, nor is a tentative definition (a
# common symbol). (ld-prime lists those too, and every label of a C
# string apart.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.data
.p2align 3
.globl _d1, _d2
_d1: .quad 1
_d2: .quad l_.s1
.cstring
l_.dead: .asciz "deadstr"
l_.s1: .asciz "dup"
.comm _zz,4,2
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.p2align 3
.globl _dead_b, _wk
.weak_definition _wk
_dead_b: .quad 1, 2
_wk: .quad 3
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.p2align 3
.globl _wk, _zz, _dead_c
.weak_definition _wk
_wk: .quad 4
_zz: .long 5
_dead_c: .long 6
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.p2align 3
.globl _p2, _dead_d
_p2: .quad l_.s2
_dead_d: .quad 0
.cstring
l_.s2: .asciz "dup"
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o -Wl,-dead_strip \
  -Wl,-u,_d2 -Wl,-u,_wk -Wl,-u,_zz -Wl,-u,_p2 -Wl,-map,$t/map
$t/exe || true

sed -n '/^# Dead Stripped Symbols:/,$p' $t/map | grep -v '^#' > $t/dead
diff - $t/dead <<EOF
<<dead>>	0x00000008	[  1] _d1
<<dead>>	0x00000008	[  1] anon
<<dead>>	0x00000010	[  2] _dead_b
<<dead>>	0x00000004	[  3] _dead_c
<<dead>>	0x00000008	[  4] _dead_d
EOF

# The ones that stay are listed live, a's string for both.
grep -q $'\t0x00000008\t\\[  1\\] _d2$' $t/map
grep -q $'\t0x00000008\t\\[  2\\] _wk$' $t/map
grep -q $'\t0x00000004\t\\[  3\\] _zz$' $t/map
grep -q $'\t0x00000008\t\\[  4\\] _p2$' $t/map
[ "$(grep -c $'\t0x00000004\t\\[  1\\] anon$' $t/map)" -eq 1 ]
