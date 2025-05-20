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

MOLD_DEBUG=1 $CC -B. -o $t/exe $t/a.o $t/b.o -g -Wl,--separate-debug-file

if readelf -S $t/exe.dbg | grep -F .debug_line_str; then
  readelf -p .debug_line_str $t/exe.dbg | grep -A10 -Fw a.c | grep -Fw b.c
fi
