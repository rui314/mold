#!/bin/bash
source "$(dirname "$0")"/common.inc

# An input section with no bytes that defines no symbol naming a
# subsection makes no output section and takes no part in ordering, in a
# final link and in -r (ld-prime). In an object with subsections the
# arm64 assembler's ltmpN labels name no subsection, so they don't keep
# one.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__zero_one
.section __DATA,__zero_two
.section __DATA,__zero_labeled
zero_label:
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__zero_two
labeled_two: .quad 3
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2; next }
    $1 == "segname" { if (s != "" && $2 == "__DATA") print s; s = "" }' | tr '\n' ' '
}

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o
[ "$(sects $t/exe)" = '__zero_labeled ' ]

# b.o's labeled __zero_two is placed as if a.o had no __zero_two.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o
[ "$(sects $t/exe2)" = '__zero_labeled __zero_two ' ]

$mold -arch $ARCH -r $t/a.o -o $t/r.o
[ "$(sects $t/r.o)" = '__zero_labeled ' ]

# Such a section can't be a relocation's target.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__e
Lin_e:
.data
.p2align 3
.globl _p
_p: .quad Lin_e
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe3 $t/main.o $t/c.o 2> $t/log
grep -Eq 'points to section\(2\) with no content|invalid r_symbolnum' $t/log
