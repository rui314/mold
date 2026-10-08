#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Local symbols in a mergeable section survive the section being split
# into fragments, even if it is not allocated in memory, like .debug_str.
cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl _start
_start:
.section .rodata.str1.1,"aMS",%progbits,1
.Lstr: .asciz "rodata"
str: .asciz "rodata2"
.section .debug_str,"MS",%progbits,1
dstr1: .asciz "first"
dstr2: .asciz "second"
.section .comment,"MS",%progbits,1
cmt: .asciz "comment"
EOF

./mold -o $t/exe $t/a.o
readelf -sW $t/exe > $t/log
grep -q ' str$' $t/log
grep -q ' dstr1$' $t/log
grep -q ' dstr2$' $t/log
grep -q ' cmt$' $t/log
not grep -q ' .Lstr$' $t/log

# Each symbol's value is the offset of its string in the merged section.
readelf -p .debug_str $t/exe > $t/str
off=$(sed -n 's/^ *\[ *\([0-9a-f]*\)\] *first$/\1/p' $t/str)
grep -Eq "^ *[0-9]+: 0*$off .* dstr1$" $t/log

./mold -r -o $t/b.o $t/a.o
readelf -sW $t/b.o > $t/log2
grep -q ' dstr1$' $t/log2
grep -q ' cmt$' $t/log2
