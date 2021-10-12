#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -fPIC -o $t/a.so -xc -
int foo() { return 3; }
EOF

cat <<EOF | cc -shared -fPIC -o $t/b.so -xc -
int bar() { return 3; }
EOF

cat <<EOF | cc -shared -fPIC -o $t/c.so -xc -
int foo();
int baz() { return foo(); }
EOF

cat <<EOF | cc -c -o $t/d.o -xc -
#include <stdio.h>
int baz();
int main() {
  printf("%d\n", baz());
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/d.o -Wl,--as-needed \
  $t/c.so $t/b.so $t/a.so

readelf --dynamic $t/exe > $t/log
grep -q /a.so $t/log
grep -q /c.so $t/log
! grep -q /b.so $t/log || false

echo OK
