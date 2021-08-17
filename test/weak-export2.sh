#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.c
#include <stdio.h>

__attribute__((weak)) extern int foo;
static int * const bar[] = {&foo};

int main() {
  printf("%d\n", bar[0] ? *bar[0] : 3);
}
EOF

cc -c -o $t/b.o $t/a.c -fno-PIC
cc -c -o $t/c.o $t/a.c -fPIC

clang -fuse-ld=$mold -no-pie -o $t/exe1 $t/b.o
clang -fuse-ld=$mold -no-pie -o $t/exe2 $t/c.o

$t/exe1 | grep -q 3
$t/exe2 | grep -q 3

cat <<EOF | cc -shared -o $t/d.so -xc -
int foo = 7;
EOF

LD_PRELOAD=$t/d.so $t/exe1 | grep -q 3
LD_PRELOAD=$t/d.so $t/exe2 | grep -q 7

echo OK
