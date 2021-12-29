#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -x assembler -
.globl x, y
.section .tbss,"awT",@nobits
x:
.zero 1024
.section .tcommon,"awT",@nobits
y:
.zero 1024
EOF

cat <<EOF | cc -c -o $t/b.o -xc -
#include <stdio.h>

extern _Thread_local char x[1024000];
extern _Thread_local char y[1024000];

int main() {
  x[0] = 3;
  x[1023] = 5;
  printf("%d %d %d %d %d %d\n", x[0], x[1], x[1023], y[0], y[1], y[1023]);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '^3 0 5 0 0 0$'

echo OK
