#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip
on_qemu && skip
command -v gdb >& /dev/null || skip
command -v flock >& /dev/null || skip

cat <<EOF > $t/a.c
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -c -o $t/a.o $t/a.c -g
$CC -B. -o $t/exe1 $t/a.o -Wl,--separate-debug-file
readelf -SW $t/exe1 | grep -F .gnu_debuglink

flock $t/exe1 true
gdb $t/exe1 -ex 'list main' -ex 'quit' | grep -F printf


$CC -c -o $t/a.o $t/a.c -g
$CC -B. -o $t/exe2 $t/a.o -Wl,--separate-debug-file,--no-build-id
readelf -SW $t/exe2 | grep -F .gnu_debuglink

flock $t/exe2 true
gdb $t/exe2 -ex 'list main' -ex 'quit' | grep -F printf


$CC -c -o $t/a.o $t/a.c -g
$CC -B. -o $t/exe3 $t/a.o -Wl,--separate-debug-file,--compress-debug-sections=zlib
readelf -SW $t/exe3 | grep -F .gnu_debuglink

flock $t/exe3 true
readelf -W --sections $t/exe3.dbg | grep '\.debug_info .*C'
gdb $t/exe3 -ex 'list main' -ex 'quit' | grep -F printf
