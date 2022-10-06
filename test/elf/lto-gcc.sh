#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $GCC -flto -o /dev/null -xc - >& /dev/null \
  || skip

cat <<EOF | $GCC -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$GCC -B. -o $t/exe -flto $t/a.o
$QEMU $t/exe | grep -q 'Hello world'
