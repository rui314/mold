#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
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

clang -fuse-ld=$mold -o $t/exe $t/a.o
! readelf --sections $t/exe | fgrep -q .repro || false


clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-repro
objcopy --dump-section .repro=$t/repro.tar.gz $t/exe

tar -C $t -xzf $t/repro.tar.gz
fgrep -q /a.o  $t/repro/response.txt
fgrep -q mold $t/repro/version.txt


MOLD_REPRO=1 clang -fuse-ld=$mold -o $t/exe $t/a.o
objcopy --dump-section .repro=$t/repro.tar.gz $t/exe

tar -C $t -xzf $t/repro.tar.gz
fgrep -q /a.o  $t/repro/response.txt
fgrep -q mold $t/repro/version.txt

echo OK
