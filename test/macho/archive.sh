#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | clang -c -o $t/c.o -xc -
void hello();

int main() {
  hello();
}
EOF

clang++ -fuse-ld=$mold -o $t/exe $t/c.o $t/b.a
$t/exe | grep -q 'Hello world'

echo OK
