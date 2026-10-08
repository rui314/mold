#!/usr/bin/env bash
. $(dirname $0)/common.inc

# With -g3, GCC puts the macro information of each header file into a
# COMDAT group. mold keeps all copies of these groups instead of
# deduplicating them, and -r output must keep each copy as a group.
cat <<EOF > $t/a.h
#define A 23
#define B 99
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -I$t -g3
#include "a.h"
extern int z();
int main () { return z() - 122; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -I$t -g3
#include "a.h"
int z()  { return A + B; }
EOF

readelf --section-groups $t/b.o | grep debug_macro || skip

./mold -r -o $t/d.o $t/b.o $t/c.o
readelf --section-groups $t/d.o > $t/log
[ $(grep -c '^COMDAT group section .* \[wm4\.a\.h\.' $t/log) = 2 ]

# Each SHF_GROUP section must be a member of a group.
readelf -SW $t/d.o | grep -E ' [A-Za-z]*G[A-Za-z]* +[0-9]+ +[0-9]+ +[0-9]+$' |
  sed -E 's/^ *\[ *([0-9]+)\].*/\1/' > $t/flagged
while read i; do grep -E "^ +\[ +$i\] " $t/log; done < $t/flagged

$CC -B. -o $t/exe $t/d.o
$QEMU $t/exe
$OBJDUMP --dwarf=macro $t/exe | grep DW_MACRO_import
$OBJDUMP --dwarf=macro $t/exe | not grep -E 'DW_MACRO_import -.* (0x)?0$'
