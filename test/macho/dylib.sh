#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.dylib -xc -
#include <stdio.h>
char world[] = "world";

char *hello() {
  return "Hello";
}
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

char *hello();
extern char world[];

int main() {
  printf("%s %s\n", hello(), world);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/b.o $t/a.dylib
$t/exe | grep -q 'Hello world'

echo OK
