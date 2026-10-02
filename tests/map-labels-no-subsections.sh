#!/bin/bash
source "$(dirname "$0")"/common.inc

# In an object without subsections, ld-prime's -map lists the labels at
# one place as in an object with them - the best to name the
# subsection first (see map-subsection-names and map-literal-labels) - but
# gives the size to the last, the worst, which names the subsection
# there; an arm64 assembler's ltmpN label is one of them.
cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 0
ka:
kb:
kc:
.quad 1
la:
lb:
.quad 2
.globl _ga
_ga:
kd:
ke:
.quad 3
.section __TEXT,__cstring,cstring_literals
.asciz "x"
zs:
as:
.asciz "y"
.data
.quad 0
.globl _da
_da:
dz:
da:
.quad 4
.text
.globl _main
_main:
  ret
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
grep -E '\] (k[a-e]|l[ab]|_ga|_da|d[az]|8-byte-literal|[az]s|literal string: y)$' $t/map |
  cut -f2- > $t/rows
diff - $t/rows <<EOF
0x00000008	[  1] 8-byte-literal
0x00000000	[  1] kc
0x00000000	[  1] kb
0x00000008	[  1] ka
0x00000000	[  1] 8-byte-literal
0x00000008	[  1] la
0x00000000	[  1] _ga
0x00000000	[  1] ke
0x00000008	[  1] kd
0x00000000	[  1] literal string: y
0x00000002	[  1] as
0x00000000	[  1] _da
0x00000000	[  1] dz
0x00000008	[  1] da
EOF

# ld-prime makes a subsection of each label at a C string's start, the
# best the string's own - known by its contents -, each other
# linker-private one a copy merged into it, and each other one an alias
# of no size. -dead_strip lists the copies as dead, and the aliases of a
# dead string or that nothing live refers to (as, but not ps).
cat <<'EOF' | $CC -o $t/b.o -c -xassembler -
.cstring
zs:
as: .asciz "two"
l_.a:
ps:
qs: .asciz "mixed"
l_.b:
l_.c:
ds:
cs: .asciz "dead"
.data
.globl _main
_main: .quad zs
.quad ps
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-map,$t/map2 -Wl,-dead_strip -nostartfiles \
  -Wl,-e,_main
grep -E 'literal string|\] [acdpqz]s$' $t/map2 > $t/rows2
diff - <(cut -f2- $t/rows2) <<EOF
0x00000004	[  1] literal string: two
0x00000006	[  1] literal string: mixed
0x00000000	[  1] ps
0x00000000	[  1] as
0x00000006	[  1] literal string: mixed
0x00000005	[  1] literal string: dead
0x00000005	[  1] literal string: dead
0x00000000	[  1] ds
0x00000000	[  1] cs
EOF

# A final image's __text that no subsection reached keeps a placeholder
# of no size, the linker's, named by the section.
echo 'int x = 1;' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -shared -o $t/c.dylib $t/c.o -Wl,-map,$t/map3
grep -q $'^0x[0-9A-F]*\t0x00000000\t\\[  0\\] __TEXT,__text$' $t/map3

# ld-prime lists the rows at one address as the label naming a
# subsection with bytes, then those naming empty ones, then the other
# labels, by their subsections' order (the end labels of the one
# before first). An object without subsections has one subsection per
# section, whose size ld-prime splits among its labels only when
# nothing comes between them: else the first label keeps it all.
cat <<'EOF' | $CC -o $t/d.o -c -xassembler -
.data
.globl _d0, _d_end1, _d_end2
_d0: .quad 0
_d_end1:
_d_end2:
EOF
printf '.data\n.globl _e\n_e:\n' | $CC -o $t/e.o -c -xassembler -
cat <<'EOF' | $CC -o $t/f.o -c -xassembler -
.data
.globl _f1, _f2, _f3
_f1:
_f2: .quad 1
_f3: .quad 2
EOF
cat <<'EOF' | $CC -o $t/g.o -c -xassembler -
.data
.globl _g
_g: .quad 3
.subsections_via_symbols
EOF
$CC --ld-path=$mold -shared -o $t/d.dylib $t/d.o $t/e.o $t/f.o $t/g.o -Wl,-map,$t/map4
grep -E '\] (_d_end.|_[efg].?)$' $t/map4 | cut -f2- > $t/rows4
diff - $t/rows4 <<EOF
0x00000010	[  3] _f2
0x00000000	[  2] _e
0x00000000	[  1] _d_end2
0x00000000	[  1] _d_end1
0x00000000	[  3] _f1
0x00000000	[  3] _f3
0x00000008	[  4] _g
EOF

$CC --ld-path=$mold -shared -o $t/e.dylib $t/d.o $t/g.o -Wl,-map,$t/map5
grep -E '\] (_d_end.|_g)$' $t/map5 | cut -f2- > $t/rows5
diff - $t/rows5 <<EOF
0x00000008	[  2] _g
0x00000000	[  1] _d_end2
0x00000000	[  1] _d_end1
EOF
