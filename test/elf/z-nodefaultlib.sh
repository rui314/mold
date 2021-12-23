#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-z,nodefaultlib
readelf --dynamic $t/exe | grep -q 'Flags: NODEFLIB'

echo OK
