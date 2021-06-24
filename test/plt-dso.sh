#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

void world() {
  printf("world\n");
}

void hello() {
  printf("Hello ");
  world();
}
EOF

clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o

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

clang -fuse-ld=`pwd`/../mold -o $t/exe -Wl,-rpath=$t $t/c.o $t/b.so
$t/exe | grep -q 'Hello WORLD'

echo OK
