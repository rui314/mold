#!/bin/bash
source "$(dirname "$0")"/common.inc

# An arm64 kext's code goes to __TEXT_EXEC,__text (-text_exec): every
# section of pure instructions, whatever its name or segment, and
# before -rename_section, which so cannot catch it under its input
# name. A section with only some instructions stays where it is.
[ $ARCH = arm64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xc -O1 -mkernel -
int kext_start(void) { return 0; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__mycode,regular,pure_instructions
.globl _mc
_mc:
  ret
.section __DATA,__dcode,regular,pure_instructions
.globl _dc
_dc:
  ret
.section __TEXT,__some
.globl _so
_so:
  ret
EOF

$mold -arch $ARCH -kext $t/a.o $t/b.o -o $t/kext -rename_section __TEXT __mycode __BAR __m
nm -m $t/kext > $t/nm
grep -q '(__TEXT_EXEC,__text) external _mc$' $t/nm
grep -q '(__TEXT_EXEC,__text) external _dc$' $t/nm
grep -q '(__TEXT_EXEC,__text) external _kext_start$' $t/nm
grep -q '(__TEXT,__some) external _so$' $t/nm
otool -l $t/kext > $t/lc
not grep -q 'sectname __mycode\|sectname __dcode\|segname __BAR' $t/lc
