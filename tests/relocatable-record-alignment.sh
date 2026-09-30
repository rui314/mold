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
