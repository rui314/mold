#!/bin/bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep -q '__tsan_init' && skip

cat <<EOF | $CC -c -o $t/a.o -xc - -g
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc - -g
void hello();
int main() { hello(); }
EOF

./mold --relocatable -o $t/c.o $t/a.o $t/b.o

$CC -B. -o $t/exe $t/c.o
$QEMU $t/exe | grep -q 'Hello world'

$OBJDUMP --dwarf=info $t/c.o > /dev/null 2> $t/log
! grep -q Warning $t/log || false
