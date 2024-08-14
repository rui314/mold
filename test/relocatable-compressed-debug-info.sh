#!/bin/bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep -q '__tsan_init' && skip

echo 'int main() {}' | $GCC -o /dev/null -xc -g3 -gz - >& /dev/null || skip

cat <<EOF | $GCC -c -o $t/a.o -xc - -g3 -gz
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

cat <<EOF | $GCC -c -o $t/b.o -xc - -g3 -gz
void hello();
int main() { hello(); }
EOF

./mold --relocatable -o $t/c.o $t/a.o $t/b.o
$CC -B. -o $t/exe $t/c.o
$QEMU $t/exe | grep -q 'Hello world'
