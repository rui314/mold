#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -fPIC -o $t/a.o -x assembler -
.globl main
.align 8192
main:
  call hello
  xor %eax, %eax
  ret
EOF

cat <<EOF | clang -c -fPIC -o $t/b.o -xc -
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}
EOF

clang -fPIE -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
