#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib
dwarfdump $t/exe > $t/log
fgrep -q '.debug_info SHF_COMPRESSED' $t/log
fgrep -q '.debug_str SHF_COMPRESSED' $t/log

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib-gnu
dwarfdump $t/exe > $t/log
fgrep -q .zdebug_info $t/log
fgrep -q .zdebug_str $t/log

echo OK
