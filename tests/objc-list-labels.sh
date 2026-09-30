#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime names no entry of __objc_classlist and the other class and
# category lists, nor of __objc_clsrolist: of the symbols naming an
# entry it takes one for the name of its atom, which is lost - an
# external (global or private external) one if any, else the greatest
# name - and keeps the others as aliases, but for the arm64
# assembler's ltmpN labels. So clang's l_OBJC_LABEL_CLASS_$ alone
# vanishes, but stays in a -r output where a private external names
# the entry too, which vanishes instead.
entry() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.section __DATA,__objc_const
.p2align 3
_ro$1: .space 72
_metaro$1: .space 72
.section __DATA,__objc_data
.p2align 3
_cls$1: .quad _meta$1, 0, 0, 0, _ro$1
_meta$1: .quad 0, 0, 0, 0, _metaro$1
.section __DATA,__objc_classlist,regular,no_dead_strip
.p2align 3
$2
.quad _cls$1
.subsections_via_symbols
EOF
}
entry a 'l_OBJC_LABEL_CLASS_$:
.private_extern _pe_a
.globl _pe_a
_pe_a:'
entry b '_loc_b:
.private_extern _pe_b
.globl _pe_b
_pe_b:'
entry c '_loc_c:'
entry d '_aaa_d:
_zzz_d:'
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/b.o $t/c.o $t/d.o
$t/exe
nm -m $t/exe > $t/nm
grep -q '(__DATA_CONST,__objc_classlist) non-external _loc_b$' $t/nm
grep -q '(__DATA_CONST,__objc_classlist) non-external _aaa_d$' $t/nm
not grep -q '_pe_\|_loc_c\|_zzz_d' $t/nm

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o $t/c.o $t/d.o
nm -m $t/r.o > $t/nm-r
grep -q '(__DATA,__objc_classlist) non-external l_OBJC_LABEL_CLASS_\$$' $t/nm-r
grep -q '(__DATA,__objc_classlist) non-external _loc_b$' $t/nm-r
grep -q '(__DATA,__objc_classlist) non-external _aaa_d$' $t/nm-r
not grep -q '_pe_\|_loc_c\|_zzz_d' $t/nm-r

# The same goes for the list of class_ro_t records.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __DATA,__objc_const
.p2align 3
_ro: .space 72
.section __DATA,__objc_clsrolist,regular,no_dead_strip
.p2align 3
_clsro:
.quad _ro
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/e.o
nm -m $t/r2.o > $t/nm-r2
not grep -q _clsro $t/nm-r2
