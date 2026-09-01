#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An object file with 65280 or more sections uses the extended section
# numbering, in which e_shnum is zero and the actual number of sections
# is stored in the sh_size field of the first section header. GCC emits
# one LTO section per function, so a large translation unit compiled
# with -flto easily exceeds the limit.

echo 'int main() {}' | $GCC -B. -flto -o /dev/null -xc - >& /dev/null || skip

# A slim LTO object. Compile with -O1 so that WPA drops the unreferenced
# functions instead of compiling all of them at link time.
seq 1 70000 | sed 's/.*/int f&() { return &; }/' > $t/a.c
echo 'int main() { return f1() - 1; }' >> $t/a.c
$GCC -O1 -flto -fno-fat-lto-objects -c -o $t/a.o $t/a.c
readelf -h $t/a.o | grep -q 'Number of section headers: *0 '

$GCC -B. -o $t/exe1 -flto $t/a.o
$QEMU $t/exe1

# A FAT LTO object. If mold does not recognize it as an LTO object, it is
# silently linked as a regular object, so check that LTO actually ran.
seq 1 70000 | sed 's/.*/int v& = &;/' > $t/b.c
echo 'int main() { return v1 - 1; }' >> $t/b.c
$GCC -flto -ffat-lto-objects -fdata-sections -c -o $t/b.o $t/b.c
readelf -h $t/b.o | grep -q 'Number of section headers: *0 '

$GCC -B. -o $t/exe2 $t/b.o --verbose |& grep -- -fwpa
$QEMU $t/exe2
