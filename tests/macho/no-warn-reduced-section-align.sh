#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section aligned beyond its segment's alignment gets the segment's,
# with a warning that -no_warn_reduced_section_align silences, wherever
# it is given. -sectalign's own warning about lowering an alignment
# stays.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__big
.p2align 16
.globl _big
_big: .quad 1
EOF
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log
grep -q 'reducing alignment of section __DATA,__big from 0x10000 to 0x[14]000 because it exceeds segment maximum alignment' $t/log
cp $t/exe $t/exe0

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-no_warn_reduced_section_align 2> $t/log
not grep -q 'reducing alignment' $t/log
cmp $t/exe $t/exe0

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-sectalign,__DATA,__big,8 \
  -Wl,-no_warn_reduced_section_align 2> $t/log
grep -q -- '-sectalign reduces alignment of __DATA,__big from 65536 to 8' $t/log
