#!/bin/bash
source "$(dirname "$0")"/common.inc

# In a -r output, ld64 names each selector reference and CFString
# constant itself (l<n>); mold used to crash on an input's own symbol
# at one. (ld-prime drops such a symbol; mold keeps it, as an alias.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__objc_methname,cstring_literals
L_m1: .asciz "m1"
L_m2: .asciz "m2"
.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip
.p2align 3
.globl _r1
_r1: .quad L_m1
_r2: .quad L_m2
.section __TEXT,__cstring,cstring_literals
L_s: .asciz "hi"
.section __DATA,__cfstring
.p2align 3
.globl _c1
_c1: .quad ___CFConstantStringClassReference
.long 1992
.space 4
.quad L_s
.quad 2
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/r.o $t/a.o
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework CoreFoundation -lobjc
$RUN $t/exe
