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
$RUN $t/exe

# __LD,__compact_unwind is aligned for its records' pointers, whatever
# the inputs' alignment, and a later link reads the same unwind info
# from it. (ld-prime keeps the largest of the inputs' alignments.)
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
$mold -r -arch $ARCH -o $t/r2.o $t/b.o $t/c.o
otool -l $t/r2.o | grep -A5 'sectname __compact_unwind$' | grep -q 'align 2^3 '
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b.o $t/c.o
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/r2.o
otool -s __TEXT __unwind_info $t/exe2 | tail -n +2 > $t/unwind2
otool -s __TEXT __unwind_info $t/exe3 | tail -n +2 > $t/unwind3
diff $t/unwind2 $t/unwind3
