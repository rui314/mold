#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | clang -B. -flto -o /dev/null -xc - >& /dev/null || skip

cat <<EOF | clang -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -B. -o $t/exe -flto $t/a.o
$t/exe | grep 'Hello world'

# LLVMgold passes a null symbol array for a file with no symbols.
echo | clang -flto -c -o $t/b.o -xc -
clang -B. -o $t/exe -flto $t/a.o $t/b.o
$t/exe | grep 'Hello world'
