#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

ldd $mold-wrapper.so | grep -q libasan && { echo skipped; exit; }

cat <<'EOF' | cc -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

gcc -fuse-ld=bfd -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log || false

clang -fuse-ld=bfd -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log || false

LD_PRELOAD=$mold-wrapper.so MOLD_PATH=$mold \
  gcc -o $t/exe $t/a.o -B/usr/bin
readelf -p .comment $t/exe > $t/log
grep -q mold $t/log

LD_PRELOAD=$mold-wrapper.so MOLD_PATH=$mold \
  clang -o $t/exe $t/a.o -fuse-ld=/usr/bin/ld
readelf -p .comment $t/exe > $t/log
grep -q mold $t/log

$mold -run env | grep -q '^MOLD_PATH=.*/mold$'

$mold -run /usr/bin/ld --version | grep -q mold
$mold -run /usr/bin/ld.lld --version | grep -q mold
$mold -run /usr/bin/ld.gold --version | grep -q mold

echo OK
