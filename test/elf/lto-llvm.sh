#!/bin/bash
dirname=$(dirname "$0")
source $dirname/base.sh

[ $MACHINE = $(uname -m) ] || { echo skipped; exit; }

echo 'int main() {}' | clang -flto -o /dev/null -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | clang -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -B. -o $t/exe -flto $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
