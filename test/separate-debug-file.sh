#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep -q '__tsan_init' && skip
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
readelf -SW $t/exe1 | grep -Fq .gnu_debuglink

flock $t/exe1 true
gdb $t/exe1 -ex 'list main' -ex 'quit' | grep -Fq printf

$CC -c -o $t/a.o $t/a.c -g
$CC -B. -o $t/exe2 $t/a.o -Wl,--separate-debug-file -Wl,--no-build-id
readelf -SW $t/exe2 | grep -Fq .gnu_debuglink

flock $t/exe2 true
gdb $t/exe2 -ex 'list main' -ex 'quit' | grep -Fq printf
