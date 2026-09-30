#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section("__DATA,__blob"))) char blob[10] = "hi";
__attribute__((section("__DATA,__vec"), aligned(16))) char vec[16] = "hi";
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x1000
otool -l $t/exe > $t/log
# The section is 2^12-aligned.
grep -A6 'sectname __blob' $t/log | grep 'align 2\^12'
addr=$(grep -A2 'sectname __blob' $t/log | awk '/addr/{print $2}')
[ $(( addr % 0x1000 )) -eq 0 ]

# An alignment that is not a power of two stands for the largest power
# of two that divides it. The first -sectalign for a section counts.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x300 \
  -Wl,-sectalign,__DATA,__blob,0x1000 2> $t/log2
grep -q 'alignment for -sectalign __DATA __blob is not a power of two, using 0x100' $t/log2
otool -l $t/exe | grep -A6 'sectname __blob' | grep 'align 2\^8'

# Only a -w before the option silences that warning.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-w -Wl,-sectalign,__DATA,__blob,6 2> $t/log2
not grep -q 'not a power of two' $t/log2

# -sectalign lowers an alignment too, with a warning.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__vec,4 2> $t/log2
grep -q -- '-sectalign reduces alignment of __DATA,__vec from 16 to 4' $t/log2
otool -l $t/exe | grep -A6 'sectname __vec' | grep 'align 2\^2'
