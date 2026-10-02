#!/bin/bash
source "$(dirname "$0")"/common.inc

# A tentative definition (a common symbol) in a live file gives way to
# an archive member that defines the symbol as data, which it loads,
# whatever the sizes, but not to one that defines it in code, nor to a
# dylib that comes first (ld-prime, as ld64 looks for a "data
# definition" in the archives for each tentative one).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.comm _x,16,4
EOF
cat <<EOF | $CC -o $t/data.o -c -xassembler -
.data
.globl _x, _data_member
_data_member: .long 0
_x: .long 42
EOF
cat <<EOF | $CC -o $t/code.o -c -xassembler -
.text
.globl _x, _code_member
_code_member: ret
_x: ret
EOF
cat <<EOF | $CC -o $t/dylib.o -c -xassembler -
.data
.globl _x
_x: .quad 0
EOF
rm -f $t/libdata.a $t/libcode.a
ar rcs $t/libdata.a $t/data.o
ar rcs $t/libcode.a $t/code.o
$CC --ld-path=$mold -shared -o $t/libx.dylib $t/dylib.o

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o $t/libcode.a $t/libdata.a
nm -m $t/b.dylib > $t/log1
grep -q ' (__DATA,__data) external _x$' $t/log1
grep -q ' _data_member$' $t/log1
not grep -q _code_member $t/log1

$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/libcode.a
nm -m $t/c.dylib > $t/log2
grep -q ' (__DATA,__common) external _x$' $t/log2
not grep -q _code_member $t/log2

$CC --ld-path=$mold -shared -o $t/d.dylib $t/a.o $t/libx.dylib $t/libdata.a
nm -m $t/d.dylib > $t/log3
grep -q ' (__DATA,__data) external _x$' $t/log3

# A member's own tentative definition is a definition that a reference
# loads it for, though ar leaves it out of the archive's table of
# contents, and like any it gives way to a member that defines the
# symbol as data, not to one with code.
cat <<EOF | $CC -o $t/ref.o -c -xassembler -
.data
.globl _ref
_ref: .quad _x
EOF
cat <<EOF | $CC -o $t/tent.o -c -xassembler -
.comm _x,32,3
.data
.globl _tent_member
_tent_member: .long 0
EOF
rm -f $t/libtent.a
ar rcs $t/libtent.a $t/tent.o

$CC --ld-path=$mold -shared -o $t/e.dylib $t/ref.o $t/libtent.a $t/libcode.a
nm -m $t/e.dylib > $t/log4
grep -q ' (__DATA,__common) external _x$' $t/log4
grep -q ' _tent_member$' $t/log4
not grep -q _code_member $t/log4

$CC --ld-path=$mold -shared -o $t/f.dylib $t/ref.o $t/libtent.a $t/libdata.a
nm -m $t/f.dylib > $t/log5
grep -q ' (__DATA,__data) external _x$' $t/log5
grep -q ' _tent_member$' $t/log5
grep -q ' _data_member$' $t/log5
