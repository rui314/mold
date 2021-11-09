#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.dylib -xc -
_Thread_local int a;
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

extern _Thread_local int a;

int main() {
  printf("%d\n", a);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.dylib $t/b.o
$t/exe | grep -q '^0$'

echo OK
