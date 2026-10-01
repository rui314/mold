#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes each -sectcreate and -add_empty_section option a file
# of one section in the option's place among the inputs - but the first
# option's section, which it takes for its own (file 0) and places
# after every file's. Options naming one section make one section of
# their contents, and one naming an input's section joins it, in file
# order.
cat <<EOF | $CC -o $t/a.o -c -xc -
int a = 0x11111111;
int main() { return a; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int b = 0x22222222;
int bfn(void) { return b; }
EOF
printf 'AAAA' > $t/d1
printf 'BBBB' > $t/d2
printf 'CCCC' > $t/d3

# One __NEW,__x of the three options' contents, the first option's
# last.
$mold -arch $ARCH -o $t/exe1 $t/a.o -lSystem -syslibroot "$(xcrun --show-sdk-path)" \
  -sectcreate __NEW __x $t/d1 -sectcreate __NEW __x $t/d2 -sectcreate __NEW __x $t/d3
otool -l $t/exe1 | grep -c 'sectname __x' > $t/log1
grep -qx 1 $t/log1
otool -X -s __NEW __x $t/exe1 | cut -f2 | tr -d ' \n' > $t/log2
grep -qx 424242424343434341414141 $t/log2

# __DATA,__data: a's, d2's (before b.o), b's, then d1's.
$mold -arch $ARCH -o $t/exe2 $t/a.o -sectcreate __DATA __data $t/d1 \
  -sectcreate __DATA __data $t/d2 $t/b.o -lSystem \
  -syslibroot "$(xcrun --show-sdk-path)" -map $t/map2
otool -l $t/exe2 | grep -c 'sectname __data' > $t/log3
grep -qx 1 $t/log3
otool -X -s __DATA __data $t/exe2 | cut -f2 | tr -d ' \n' > $t/log4
grep -qx 11111111424242422222222241414141 $t/log4

# The map lists the second option's file where it stands, the first's
# contents as the linker's.
grep -q '^\[  1\] .*/a.o$' $t/map2
grep -q '^\[  2\] .*/d2$' $t/map2
grep -q '^\[  3\] .*/b.o$' $t/map2
grep -q $'\t0x00000004\t\[  2\] l<sect-create>__DATA,__data$' $t/map2
grep -q $'\t0x00000004\t\[  0\] l<sect-create>__DATA,__data$' $t/map2

# An empty section's file is "(null)" in the map, as ld-prime has it.
$mold -arch $ARCH -o $t/exe3 $t/a.o -sectcreate __NEW __x $t/d1 \
  -add_empty_section __NEW __e -lSystem -syslibroot "$(xcrun --show-sdk-path)" -map $t/map3
grep -q '^\[  2\] (null)$' $t/map3
grep -q $'\t0x00000000\t\[  2\] l<sect-create>__NEW,__e$' $t/map3

# Code in __text: one __text, the contents after main's.
$mold -arch $ARCH -o $t/exe4 $t/a.o -sectcreate __TEXT __text $t/d1 \
  -lSystem -syslibroot "$(xcrun --show-sdk-path)"
otool -l $t/exe4 | grep -c 'sectname __text' > $t/log5
grep -qx 1 $t/log5
$t/exe4 || [ $? = 17 ]
