#!/bin/bash
set -e
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

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-repro
objcopy --dump-section .repro=$t/repro.tar $t/exe

tar -C $t -xf $t/repro.tar
fgrep -q /a.o  $t/repro/response.txt
fgrep -q mold $t/repro/version.txt

echo OK
