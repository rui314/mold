#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip
command -v flock >& /dev/null || skip

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
flock $t/exe.dbg true
readelf -p .debug_line_str $t/exe.dbg > $t/str

if grep -Fw a.c $t/str; then
  grep -A10 -Fw a.c $t/str | grep -Fw b.c
fi
