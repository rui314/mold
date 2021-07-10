#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.so -xc -
void fn() {}
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

void fn();
void (*ptr)() = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/b.o $t/a.so
$t/exe | grep -q 1

echo OK
