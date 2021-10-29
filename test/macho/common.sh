#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -fcommon -c -xc -
int foo;
int bar;
EOF

cat <<EOF | cc -o $t/b.o -fcommon -c -xc -
int foo;
int bar = 5;
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;
static int baz[10000];

int main() {
  printf("%d %d %d\n", foo, bar, baz[0]);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^0 5 0$'

echo OK
