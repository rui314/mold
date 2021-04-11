#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -ffunction-sections -fdata-sections -xc -
#include <stdio.h>

int bar() {
  return 5;
}

int foo1(int x) {
  return bar() + x;
}

int foo2(int x) {
  return bar() + x;
}

int foo3() {
  bar();
  return 5;
}

int main() {
  printf("%d %d\n", (long)foo1 == (long)foo2, (long)foo1 == (long)foo3);
  return 0;
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-icf
$t/exe | grep '1 0'

echo OK
