#!/bin/bash
source "$(dirname "$0")"/common.inc

# The __LD segment carries data for the linker itself, and ld-prime
# reads only __LD,__compact_unwind: any other section there is dropped
# with a warning naming it and the object's real path - also in an
# archive member the link doesn't use, and in -r - and a relocation to
# a symbol defined in one is an error.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __LD,__ll
.quad 1
.section __LD,__zz,regular,no_dead_strip
.quad 2
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __LD,__mm
.quad 3
.text
.globl _unused_member
_unused_member: ret
.subsections_via_symbols
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/b.o

$CC --ld-path=$mold -o $t/exe $t/a.o $t/lib.a 2> $t/log
grep -q 'unknown section: __LD/__ll in /.*/a.o$' $t/log
grep -q 'unknown section: __LD/__zz in /.*/a.o$' $t/log
grep -q 'unknown section: __LD/__mm in /.*/lib.a\[2\](b.o)$' $t/log
not grep -q 'compact_unwind' $t/log
otool -l $t/exe > $t/lc
not grep -q 'segname __LD' $t/lc

$mold -arch $ARCH -r $t/a.o -o $t/r.o 2> $t/log2
grep -q 'unknown section: __LD/__ll' $t/log2
otool -l $t/r.o > $t/lc2
not grep -q '__ll' $t/lc2

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __LD,__ll
.globl _llsym
_llsym: .quad 1
.data
.globl _p
_p: .quad _llsym
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/c.o 2> /dev/null
