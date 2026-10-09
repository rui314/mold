#!/bin/bash
source "$(dirname "$0")"/common.inc

# A rename that puts zero-fill and file-backed input sections in one
# output section draws a warning, and the section is file-backed: the
# zero-fill members are zeros in the file, and no member's contents are
# lost, whichever member comes first. (ld-prime gives the section its
# first member's type in its own order, dropping the file-backed
# members' contents if that is zero fill.)
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
grep -q 'warning: section __DATA,__bss has both zero-fill and file-backed' $t/log
otool -l $t/exe | grep -A9 'sectname __bss' | grep -q 'flags 0x00000000'
otool -X -s __DATA __bss $t/exe | cut -f2 | tr -d ' \n' > $t/bss
grep -Eqx '0{128}00000003000000000000000400000000|0{128}03000000000000000400000000000000' $t/bss

# Not in a -r output.
$mold -r -arch $ARCH -o $t/c.o $t/a.o $t/b.o -rename_section __DATA __data __DATA __bss \
  2> $t/log2
not grep -q zero-fill $t/log2

# A common symbol's zero fill is no different, before or after the
# file-backed section.
cat <<EOF2 | $CC -o $t/d.o -c -xassembler -
.comm _common_sym,16,3
.text
.globl _main
_main:
  ret
EOF2

$mold -arch $ARCH -static -e _main -o $t/exe3 $t/d.o $t/b.o \
  -rename_section __DATA __common __DATA __data 2> $t/log3
grep -q 'warning: section __DATA,__data has both zero-fill and file-backed' $t/log3
otool -l $t/exe3 | grep -A9 'sectname __data' | grep -q 'flags 0x00000000'
otool -l $t/exe3 | grep -A3 'sectname __data' | grep -q 'size 0x0*18$'

$mold -arch $ARCH -static -e _main -o $t/exe4 $t/b.o $t/d.o \
  -rename_section __DATA __common __DATA __data 2> $t/log4
grep -q 'warning: section __DATA,__data has both zero-fill and file-backed' $t/log4
otool -l $t/exe4 | grep -A9 'sectname __data' | grep -q 'flags 0x00000000'
