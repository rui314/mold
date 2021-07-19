#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.so -xc -
int foo = 3;
int bar = 5;
EOF

cat <<EOF | cc -fno-PIC -c -o $t/b.o -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d\n", foo, bar);
  return 0;
}
EOF

clang -fuse-ld=$mold -no-pie -o $t/exe $t/a.so $t/b.o
$t/exe | grep -q '3 5'

! clang -fuse-ld=$mold -o $t/exe $t/a.so $t/b.o \
  -Wl,-z,nocopyreloc 2> $t/log || false

grep -q 'recompile with -fPIE' $t/log

echo OK
