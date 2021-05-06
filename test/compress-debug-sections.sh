#!/bin/bash
set -e
cd $(dirname $0)
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

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib
dwarfdump $t/exe > $t/log
grep -q '.debug_info SHF_COMPRESSED' $t/log
grep -q '.debug_str SHF_COMPRESSED' $t/log

echo OK
