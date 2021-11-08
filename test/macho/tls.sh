#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
_Thread_local int a;
_Thread_local int b = 5;
int c;
int d = 1;
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

extern _Thread_local int a;
extern _Thread_local int b;
extern int c;
extern int d;

int main() {
  printf("%d %d %d %d\n", a, b, c, d);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '^0 5 0 1$'

echo OK
