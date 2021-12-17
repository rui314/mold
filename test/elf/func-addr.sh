#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.so -xc -
void fn() {}
EOF

cat <<EOF | cc -o $t/b.o -c -xc -fno-PIC -
#include <stdio.h>

typedef void Func();

void fn();
Func *const ptr = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/b.o $t/a.so
$t/exe | grep -q 1

echo OK
