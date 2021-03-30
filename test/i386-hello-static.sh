#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -m32 -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -m32 -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
