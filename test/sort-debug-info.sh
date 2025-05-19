#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -g -gdwarf32 || skip
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -g -gdwarf64
int main() {}
EOF

# Test if DWARF32 precedes DWARF64 in the output .debug_info
MOLD_DEBUG=1 $CC -B. -o $t/exe1 $t/a.o $t/b.o -g -Wl,-Map=$t/map1
sed -n '/\/a\.o:(.debug_info)/,$p' $t/map1 | grep -F '/b.o:(.debug_info)'

MOLD_DEBUG=1 $CC -B. -o $t/exe2 $t/b.o $t/a.o -g -Wl,-Map=$t/map2
sed -n '/\/a\.o:(.debug_info)/,$p' $t/map2 | grep -F '/b.o:(.debug_info)'
