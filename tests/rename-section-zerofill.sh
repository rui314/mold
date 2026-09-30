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
