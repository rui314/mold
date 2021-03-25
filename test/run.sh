#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | cc -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

gcc -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log

clang -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log

LD_PRELOAD=`pwd`/../mold-wrapper.so REAL_MOLD_PATH=`pwd`/../mold \
  gcc -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log

LD_PRELOAD=`pwd`/../mold-wrapper.so REAL_MOLD_PATH=`pwd`/../mold \
  clang -o $t/exe $t/a.o
readelf -p .comment $t/exe > $t/log
! grep -q mold $t/log

echo OK
