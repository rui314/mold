#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime cuts __objc_superrefs and __objc_protorefs into a subsection
# per pointer. One a symbol names stays apart and keeps its label, in
# an image's symbol table too; the others merge by target. A -r output
# keeps every entry and label for the final link to merge.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__objc_data
.p2align 3
.globl _cls1, _cls2
_cls1: .quad 0, 0, 0, 0, 0
_cls2: .quad 0, 0, 0, 0, 0
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__objc_superrefs,regular,no_dead_strip
.p2align 3
_sup_a: .quad _cls1
l_sup_b: .quad _cls2
.quad _cls1
.section __DATA,__objc_protorefs,coalesced,no_dead_strip
.p2align 3
_pro_a: .quad _cls1
l_pro_b: .quad _cls2
.quad _cls1
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

size() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1 }
    f && $1 == "size" { print $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/c.o
$t/exe
nm -m $t/exe > $t/nm
grep -q ',__objc_superrefs) non-external _sup_a$' $t/nm
grep -q ',__objc_protorefs) non-external _pro_a$' $t/nm
[ "$(size $t/exe __objc_superrefs)" = 0x0000000000000018 ]

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm-r
grep -q '(__DATA,__objc_superrefs) non-external \[no dead strip\] _sup_a$' $t/nm-r
grep -q '(__DATA,__objc_superrefs) non-external \[no dead strip\] l_sup_b$' $t/nm-r
$CC --ld-path=$mold -o $t/exe-r $t/main.o $t/r.o $t/c.o
$t/exe-r
nm -m $t/exe-r | grep -q ',__objc_superrefs) non-external _sup_a$'
[ "$(size $t/exe-r __objc_superrefs)" = 0x0000000000000018 ]

# Of the literal-pointer type, such references are taken for class
# references: all of one target merge, whatever labels them, and no
# label survives. The output has the standard section's flags.
for n in 1 2; do
  cat <<EOF | $CC -o $t/b$n.o -c -xassembler -
.section __DATA,__objc_superrefs,literal_pointers,no_dead_strip
.p2align 3
_sup_a$n: .quad _cls1
l_sup_b$n: .quad _cls1
.quad _cls1
.quad _cls2
.subsections_via_symbols
EOF
done
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b1.o $t/b2.o $t/c.o
$t/exe2
[ "$(size $t/exe2 __objc_superrefs)" = 0x0000000000000010 ]
nm -m $t/exe2 > $t/nm2
not grep -q '_sup_a[12]' $t/nm2
$mold -r -arch $ARCH -o $t/r2.o $t/b1.o $t/b2.o
otool -l $t/r2.o | grep -A8 'sectname __objc_superrefs' | grep -q 'flags 0x10000000'
$CC --ld-path=$mold -o $t/exe2-r $t/main.o $t/r2.o $t/c.o
$t/exe2-r
