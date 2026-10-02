#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image's symbol table lists every name of an entry of
# __objc_classlist and the other class and category lists but an
# assembler temporary (clang's l_OBJC_LABEL_CLASS_$), as locals: the
# reader demotes the externals. (ld-prime names no entry: of the symbols
# naming one it takes one for the name of its subsection, which is
# lost, and keeps the others as aliases.) A -r output keeps every label:
# each marked a subsection's start in its input, and a later link
# splits the output at them.
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
for s in _pe_a _loc_b _pe_b _loc_c _aaa_d _zzz_d; do
  grep -q "(__DATA_CONST,__objc_classlist) non-external $s\$" $t/nm
done
not grep -q l_OBJC_LABEL_CLASS_ $t/nm

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o $t/c.o $t/d.o
nm -m $t/r.o > $t/nm-r
for s in 'l_OBJC_LABEL_CLASS_\$' _pe_a _loc_b _pe_b _loc_c _aaa_d _zzz_d; do
  grep -q "(__DATA,__objc_classlist) non-external .*$s\$" $t/nm-r
done
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/r.o
$t/exe2

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
grep -q '(__DATA,__objc_clsrolist) non-external .*_clsro$' $t/nm-r2
