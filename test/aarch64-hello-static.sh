#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | aarch64-linux-gnu-gcc-10 -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

aarch64-linux-gnu-gcc-10 -B`pwd`/.. -o $t/exe $t/a.o -static
readelf -a $t/exe > $t/log
grep -Pq 'Machine:\s+AArch64' $t/log

# Generated executable can't run yet
#  qemu-aarch64 -L /usr/aarch64-linux-gnu $t/exe | grep -q 'Hello world'

echo OK
