#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An object file with 65280 or more sections uses the extended section
# numbering, in which e_shnum is zero and the actual number of sections
# is stored in the sh_size field of the first section header. Append
# empty sections to GCC's LTO assembly to exceed the limit without
# compiling thousands of functions.

echo 'int main() {}' | $GCC -B. -flto -o /dev/null -xc - >& /dev/null || skip

echo 'int main() { return 0; }' > $t/a.c
seq 1 70000 | sed 's/.*/.section .padding.&,""/' > $t/padding.s

# A slim LTO object.
$GCC -S -flto -fno-fat-lto-objects -o $t/a.s $t/a.c
cat $t/padding.s >> $t/a.s
$GCC -c -o $t/a.o $t/a.s
readelf -h $t/a.o | grep -q 'Number of section headers: *0 '

$GCC -B. -o $t/exe1 -flto $t/a.o
$QEMU $t/exe1

# A FAT LTO object. If mold does not recognize it as an LTO object, it is
# silently linked as a regular object, so check that LTO actually ran.
$GCC -S -flto -ffat-lto-objects -o $t/b.s $t/a.c
cat $t/padding.s >> $t/b.s
$GCC -c -o $t/b.o $t/b.s
readelf -h $t/b.o | grep -q 'Number of section headers: *0 '

$GCC -B. -o $t/exe2 $t/b.o --verbose |& grep -- -fwpa
$QEMU $t/exe2
