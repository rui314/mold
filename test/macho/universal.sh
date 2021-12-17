#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

lipo $t/a.o -create -output $t/fat.o

cat <<EOF | cc -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
