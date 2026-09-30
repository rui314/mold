#!/bin/bash
source "$(dirname "$0")"/common.inc

# A 32-bit object's __la_symbol_ptr held the lazy pointers only dyld's
# lazy binder fills. ld-prime takes a non-empty one of that type from a
# 64-bit object for a section of fixed-size records whose size it
# doesn't know, and refuses it, in -r too. Lazy pointers under another
# name, and an empty __la_symbol_ptr, link.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__la_symbol_ptr,lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
.text
.globl _foo
_foo: ret
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> $t/log
grep -qF "unknown fixed size section __DATA,__la_symbol_ptr with content type: lazy-pointer in '$t/a.o'" $t/log
not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log
grep -qF "unknown fixed size section __DATA,__la_symbol_ptr with content type: lazy-pointer in '$t/a.o'" $t/log

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__lazy,lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
.section __DATA,__la_symbol_ptr,lazy_symbol_pointers
.text
.globl _foo
_foo: ret
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o
$mold -r -arch $ARCH -o $t/r.o $t/b.o
