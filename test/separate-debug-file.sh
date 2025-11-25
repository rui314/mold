#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip
on_qemu && skip
command -v gdb >& /dev/null || skip
command -v flock >& /dev/null || skip

# To check for corruption, we need a binary with many external symbols.
# The easiest method of doing this is generating the library
seq 1 20 | sed 's/.*/int return&(void);/' > $t/libret.h
seq 1 20 | sed 's/.*/int return&(void) { return &; }/' > $t/libret.c

cat <<EOF > $t/a.c
#include <libret.h>
int main() {
  int x;
  $(seq 1 20 | sed 's/.*/x += return&();/')
  return x;
}
EOF

$CC -c -o $t/libret.o $t/libret.c -g
$CC -B. -o $t/libret.so -shared $t/libret.o

$CC -c -o $t/a.o $t/a.c -I$t -g
$CC -B. -o $t/exe1 $t/a.o -Wl,--separate-debug-file -L$t -lret
readelf -SW $t/exe1 | grep -F .gnu_debuglink

flock $t/exe1.dbg true
gdb $t/exe1 -ex 'list main' -ex 'quit' | grep -F return1

# Ensure the internal names for headers didn't leak in (i.e. PHDR)
# and that there is no corruption (either explicitly called out
# or in the form of empty undefined symbols past the first one).
# See issue #1535 for more details
readelf -W --sections $t/exe1.dbg | not grep -E '[EPS]HDR'
readelf -W --symbols $t/exe1.dbg | not grep -E '<corrupt>'
readelf -W --symbols $t/exe1.dbg | not grep -E '^ *[0-9][0-9]:.*UND *$'


$CC -c -o $t/a.o $t/a.c -g -I$t
$CC -B. -o $t/exe2 $t/a.o -Wl,--separate-debug-file,--no-build-id -L$t -lret
readelf -SW $t/exe2 | grep -F .gnu_debuglink

flock $t/exe2.dbg true
gdb $t/exe2 -ex 'list main' -ex 'quit' | grep -F return1


$CC -c -o $t/a.o $t/a.c -g -I$t
$CC -B. -o $t/exe3 $t/a.o -Wl,--separate-debug-file,--compress-debug-sections=zlib -L$t -lret
readelf -SW $t/exe3 | grep -F .gnu_debuglink

flock $t/exe3.dbg true
readelf -W --sections $t/exe3.dbg | grep '\.debug_info .*C'
gdb $t/exe3 -ex 'list main' -ex 'quit' | grep -F return1
