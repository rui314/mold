#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

void world() {
  printf("world\n");
}

void real_hello() {
  printf("Hello ");
  world();
}

void hello() {
  real_hello();
}
EOF

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o

cat <<EOF | cc -c -o $t/c.o -xc -
#include <stdio.h>

void world() {
  printf("WORLD\n");
}

void hello();

int main() {
  hello();
}
EOF

clang -fuse-ld=$mold -o $t/exe -Wl,-rpath=$t $t/c.o $t/b.so
$t/exe | grep -q 'Hello WORLD'

echo OK
