#!/bin/bash
source "$(dirname "$0")"/common.inc

# Without .subsections_via_symbols, ld64 cannot tell where one
# subsection's code ends and the next one's begins, so it keeps every
# subsection that it cuts at the object's symbols, referenced or not, in
# any section. It still strips an unused literal, which it cuts by
# content.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main
.text
_main:
  ret
_unused_fn:
  ret
.section __TEXT,__text2,regular,pure_instructions
_unused_text2:
  ret
.section __TEXT,__const
_unused_const:
  .quad 1
.data
_unused_data:
  .quad 2
.section __TEXT,__literal8,8byte_literals
_unused_lit8:
  .quad 3
.cstring
  .asciz "unused string"
EOF

# The same shape in an object that has the flag is stripped as usual.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.subsections_via_symbols
.globl _b_used, _b_unused
.text
_b_used:
  ret
_b_unused:
  ret
.data
_b_unused_data:
  .quad 4
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-dead_strip -Wl,-u,_b_used
nm $t/exe > $t/syms
grep -q ' _unused_fn$' $t/syms
grep -q ' _unused_text2$' $t/syms
grep -q ' _unused_const$' $t/syms
grep -q ' _unused_data$' $t/syms
not grep -q ' _unused_lit8$' $t/syms
not grep -q 'unused string' $t/exe
grep -q ' _b_used$' $t/syms
not grep -q ' _b_unused$' $t/syms
not grep -q ' _b_unused_data$' $t/syms

# -why_live names the reason such a subsection is a root.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip \
  -Wl,-why_live,_unused_text2 2> $t/log
grep -q '^_unused_text2 from .*/a.o$' $t/log
grep -q '^  dont-dead-strip$' $t/log

# An archive member without the flag that nothing loads stays out; one
# that is loaded is kept whole.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.globl _c_unloaded
.text
_c_unloaded:
  ret
EOF

cat <<EOF | $CC -o $t/d.o -c -xassembler -
.globl _d_used, _d_unused
.text
_d_used:
  ret
_d_unused:
  ret
EOF

cat <<EOF | $CC -o $t/e.o -c -xassembler -
.subsections_via_symbols
.globl _main
.text
_main:
  ret
EOF

rm -f $t/lib.a
ar rcs $t/lib.a $t/c.o $t/d.o
$CC --ld-path=$mold -o $t/exe2 $t/e.o $t/lib.a -Wl,-dead_strip -Wl,-u,_d_used
nm $t/exe2 > $t/syms2
grep -q ' _d_used$' $t/syms2
grep -q ' _d_unused$' $t/syms2
not grep -q ' _c_unloaded$' $t/syms2
