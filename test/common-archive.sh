#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fcommon -xc -c -o $t/a.o -
int foo;
EOF

cat <<EOF | cc -fcommon -xc -c -o $t/b.o -
int foo = 5;
EOF

cat <<EOF | cc -fcommon -xc -c -o $t/c.o -
#include <stdio.h>

extern int foo;

int main() {
  printf("%d\n", foo);
}
EOF

ar rcs $t/d.a $t/b.o

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/c.o $t/d.a
$t/exe | grep -q 5

echo OK
