#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 aligns every initializer and terminator pointer, and every
# CFString constant, to a pointer whatever its section's header says:
# a 2^0 __mod_init_func comes out 2^3, and so does a 2^4 one, in a -r
# output as in an image.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
_fn:
  ret
.section __DATA,__mod_init_func,mod_init_funcs
.quad _fn
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 4
.quad _fn
.section __TEXT,__cstring,cstring_literals
l_.str:
.asciz "hi"
.section __DATA,__cfstring
_str:
.quad ___CFConstantStringClassReference
.long 1992
.space 4
.quad l_.str
.quad 2
.subsections_via_symbols
EOF

align() { otool -l $1 | grep -A5 "sectname $2\$" | grep -q 'align 2^3 '; }

$mold -r -arch $ARCH -o $t/r.o $t/a.o
align $t/r.o __mod_init_func
align $t/r.o __mod_term_func
align $t/r.o __cfstring

echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -framework CoreFoundation
align $t/exe __mod_term_func
align $t/exe __cfstring
$t/exe

# __LD,__compact_unwind keeps its inputs' alignment instead: each
# record is aligned as the section it came from, so the output takes
# the largest of the surviving records', 2^0 from a 2^0 input. A
# coalesced-away weak copy's record doesn't count.
rec() {
  cat <<EOF
.text
.globl _$1
$3
_$1:
  ret
.section __LD,__compact_unwind,regular,debug
$2
.quad _$1
.long 1
.long 0x02000000
.quad 0
.quad 0
.subsections_via_symbols
EOF
}
rec f '' | $CC -o $t/b.o -c -xassembler -
rec g '.p2align 2' | $CC -o $t/c.o -c -xassembler -
rec w '' '.weak_definition _w' | $CC -o $t/d.o -c -xassembler -
rec w '.p2align 4' '.weak_definition _w' | $CC -o $t/e.o -c -xassembler -
cu_align() {
  otool -l $1 | grep -A5 'sectname __compact_unwind$' | grep -q "align 2^$2 "
}

$mold -r -arch $ARCH -o $t/r2.o $t/b.o
cu_align $t/r2.o 0
$mold -r -arch $ARCH -o $t/r3.o $t/b.o $t/c.o
cu_align $t/r3.o 2
$mold -r -arch $ARCH -o $t/r4.o $t/d.o $t/e.o
cu_align $t/r4.o 0
$mold -r -arch $ARCH -o $t/r5.o $t/e.o $t/d.o
cu_align $t/r5.o 4
