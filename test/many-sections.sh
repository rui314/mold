#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

seq 1 65500 | sed 's/.*/.section .text.\0, "ax",@progbits/' > $t/a.s

cc -c -o $t/a.o $t/a.s

cat <<'EOF' | cc -c -xc -o $t/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q Hello

echo OK
