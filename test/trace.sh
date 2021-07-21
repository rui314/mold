#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-trace > $t/log
fgrep -q "$t/a.o" $t/log

echo OK
