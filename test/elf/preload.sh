#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../..//out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

rm -f $t/exe

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-preload
! test -e $t/exe || false
clang -fuse-ld=$mold -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
