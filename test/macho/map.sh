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

cat <<EOF | cc -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o -Wl,-map,$t/map

grep -Eq '^\[  0\] .*/a.o$' $t/map
grep -Eq '^\[  1\] .*/b.o$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      __TEXT  __text$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      \[  0\] _hello$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      \[  1\] _main$' $t/map

echo OK
