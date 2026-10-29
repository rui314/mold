#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A section symbol in SHN_ABS stands for its own value. We used to crash
# on such a symbol. https://github.com/rui314/mold/issues/1741

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.section .rodata.x,"a"
.Lx: .quad 42
.data
.globl ptr
ptr: .quad .Lx+8
EOF

# Move the .rodata.x section symbol (index 1) to SHN_ABS. No assembler
# emits such a symbol, so patch the file with dd.
readelf -sW $t/a.o | grep -Eq '^ +1: 0+ +0 SECTION +LOCAL +DEFAULT +[0-9]+ \.rodata\.x$' || skip
off=$(readelf -SW $t/a.o | sed 's/\[ *[0-9]*\]//' |
      awk '$1 == ".symtab" { print $4 }')
printf '\xf1\xff' | dd of=$t/a.o bs=1 seek=$((16#$off + 24 + 6)) conv=notrunc status=none
readelf -sW $t/a.o | grep -Eq '^ +1: .* SECTION .* ABS' || skip

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
extern long ptr;
int main() { printf("%ld\n", ptr); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -x 8

./mold -r -o $t/c.o $t/a.o
$CC -B. -o $t/exe2 $t/c.o $t/b.o
$QEMU $t/exe2 | grep -x 8
