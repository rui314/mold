#!/bin/bash
source "$(dirname "$0")"/common.inc

# A rename that puts zero-fill and file-backed input sections in one
# output section draws ld-prime's warning, naming the first file with
# a zero-fill one and, the last first, the files with file-backed
# ones. The section takes its first member's type: here zero-fill.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.zerofill __DATA,__bss,_z,64,4
.text
.globl _main
_main:
  ret
.data
.quad 3
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.quad 4
EOF

$mold -arch $ARCH -static -e _main -o $t/exe $t/a.o $t/b.o \
  -rename_section __DATA __data __DATA __bss 2> $t/log
grep -A2 'warning: section __DATA,__bss has a conflicting zerofill flag defined in' $t/log \
  | sed 's|.*/||' > $t/files
[ "$(tr '\n' ' ' < $t/files)" = 'a.o but missing in: b.o a.o ' ]
otool -l $t/exe | grep -A9 'sectname __bss' | grep -q 'flags 0x00000001'

# Not in a -r output.
$mold -r -arch $ARCH -o $t/c.o $t/a.o $t/b.o -rename_section __DATA __data __DATA __bss \
  2> $t/log2
not grep -q zerofill $t/log2

# A common symbol counts as the tentative definition of the object
# declaring the largest size, after that object's own sections: first
# in the input, it makes the merged section zero-fill.
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.comm _common_sym,16,3
.text
.globl _main
_main:
  ret
EOF2

$mold -arch $ARCH -static -e _main -o $t/exe3 $t/d.o $t/b.o \
  -rename_section __DATA __common __DATA __data 2> $t/log3
grep -A1 'warning: section __DATA,__data has a conflicting zerofill flag defined in' $t/log3 \
  | sed 's|.*/||' > $t/files3
[ "$(tr '\n' ' ' < $t/files3)" = 'd.o but missing in: b.o ' ]
otool -l $t/exe3 | grep -A9 'sectname __data' | grep -q 'flags 0x00000001'

$mold -arch $ARCH -static -e _main -o $t/exe4 $t/b.o $t/d.o \
  -rename_section __DATA __common __DATA __data 2> $t/log4
grep -q 'conflicting zerofill flag defined in .*/d.o but missing in:' $t/log4
otool -l $t/exe4 | grep -A9 'sectname __data' | grep -q 'flags 0x00000000'
