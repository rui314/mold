#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | clang -flto -o /dev/null -xc - >& /dev/null \
  || skip

cat <<EOF | clang -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -B. -o $t/exe -flto $t/a.o
$t/exe | grep -q 'Hello world'
