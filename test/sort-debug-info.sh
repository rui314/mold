#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
int x = 1;
void foo() {}
EOF

$CC -o $t/a.o -c -g -gdwarf32 $t/a.c || skip

cat <<EOF > $t/b.c
int y = 1;
int main() {}
EOF

$CC -o $t/b.o -c -g -gdwarf64 $t/b.c

# Test if DWARF32 precedes DWARF64 in the output .debug_info

MOLD_DEBUG=1 $CC -B. -o $t/exe1 $t/a.o $t/b.o -g -Wl,-Map=$t/map1
grep -A10 -F '/a.o:(.debug_info)' $t/map1 | grep -F '/b.o:(.debug_info)'

if readelf -S $t/exe2 | grep -F .debug_line_str; then
  readelf -p .debug_line_str $t/exe1 | grep -A10 -Fw a.c | grep -Fw b.c
fi


MOLD_DEBUG=1 $CC -B. -o $t/exe2 $t/b.o $t/a.o -g -Wl,-Map=$t/map2
grep -A10 -F '/a.o:(.debug_info)' $t/map2 | grep -F '/b.o:(.debug_info)'

if readelf -S $t/exe2 | grep -F .debug_line_str; then
  readelf -p .debug_line_str $t/exe2 | grep -A10 -Fw a.c | grep -Fw b.c
fi


if [ -z "$QEMU" ] && command -v gdb; then
  gdb $t/exe1 -ex 'list main' -ex 'quit' | grep 'int y = 1'
  gdb $t/exe2 -ex 'list main' -ex 'quit' | grep 'int y = 1'
fi
