#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();
__attribute__((weak)) int bar();

int main() {
  printf("%d %d\n", foo ? foo() : 3, &bar ? bar() : 5);
}
EOF

cat <<EOF | cc -shared -fPIC -o $t/b.so -xc -
int foo() {
  return 42;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.so

readelf --dyn-syms $t/exe | grep -q 'FUNC    WEAK   DEFAULT  UND foo'
! readelf --dyn-syms $t/exe | grep -q 'FUNC    WEAK   DEFAULT  UND bar' || false

$t/exe | grep -q '^42 5$'

cat <<EOF | cc -shared -fPIC -o $t/b.so -xc -
EOF

$t/exe | grep -q '^3 5$'

echo OK
