#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image orders a segment's sections by fixed ranks for the ones
# ld-prime knows and by input order for the rest. In __TEXT, __text
# leads and the other code sections follow it in input order, ahead of
# the stubs and every data section; in __DATA the Objective-C sections
# lead and __data takes its input place among the unknown sections.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__zz
.quad 1
.section __DATA,__yy
.quad 1
.section __TEXT,__zcode,regular,pure_instructions
_zc: ret
.data
.quad 2
.section __DATA,__objc_data
.quad 3
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
otool -l $t/exe | awk '$1 == "sectname" { s = $2; next }
  $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }' > $t/order
grep -q '__TEXT,__text __TEXT,__zcode __TEXT,__zz ' $t/order
grep -q '__DATA,__objc_data __DATA,__yy __DATA,__data ' $t/order

# A section is placed by the first atom the output keeps: a copy of a
# string merged into another object's, more aligned one places nothing,
# nor does the losing copy of a weak definition.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__aaa
.quad 1
.section __TEXT,__cstring,cstring_literals
L1: .asciz "abc"
.section __DATA,__dda
.quad 1
.section __DATA,__weak
.globl _w
.weak_definition _w
_w: .quad 1
.data
.p2align 3
.globl _p1
_p1: .quad L1, _w
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __TEXT,__bbb
.quad 2
.section __TEXT,__cstring,cstring_literals
.p2align 4
L2: .asciz "abc"
.section __DATA,__ddb
.quad 2
.section __DATA,__weak
.p2align 4
.globl _w
.weak_definition _w
_w: .quad 2
.data
.p2align 3
.globl _p2
_p2: .quad L2, _w
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/c.o
otool -l $t/exe2 | awk '$1 == "sectname" { s = $2; next }
  $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }' > $t/order2
grep -q '__TEXT,__aaa __TEXT,__bbb __TEXT,__cstring ' $t/order2
grep -q '__DATA,__dda __DATA,__ddb __DATA,__weak ' $t/order2

# Code leads any other segment too, ahead of the sections of fixed
# ranks such as __objc_data, in input order: outside __TEXT, __text is
# code like any other.
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.section __DATA,__data
.quad 1
.section __DATA,__mycode,regular,pure_instructions
_dc: ret
.section __FOO,__x
.quad 1
.section __FOO,__b,regular,pure_instructions
_fb: ret
.section __FOO,__text,regular,pure_instructions
_ft: ret
.subsections_via_symbols
EOF2
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/d.o
otool -l $t/exe3 | awk '$1 == "sectname" { s = $2; next }
  $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }' > $t/order3
grep -q '__DATA,__mycode __DATA,__objc_data ' $t/order3
grep -q '__FOO,__b __FOO,__text __FOO,__x ' $t/order3
