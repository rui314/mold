#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.c
#include <stdio.h>

__attribute__((weak)) int foo();
__attribute__((weak)) extern int bar;

int main() {
  printf("%d %d\n", foo ? foo() : 3, &bar ? bar : 5);
}
EOF

cc -fno-PIC -c -o $t/c.o $t/a.c
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.o
! readelf --dyn-syms $t/exe | grep -q 'NOTYPE  WEAK   DEFAULT  UND foo' || false
$t/exe | grep -q '3 5'

cc -fPIC -c -o $t/b.o $t/a.c
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/b.o
readelf --dyn-syms $t/exe | grep -q 'NOTYPE  WEAK   DEFAULT  UND foo'
$t/exe | grep -q '3 5'

cat <<EOF | cc -shared -o $t/d.so -xc -
int foo() { return 42; }
int bar = 7;
EOF

LD_PRELOAD=$t/d.so $t/exe | grep -q '42 7'

echo OK
