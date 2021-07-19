#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -fno-PIC -o $t/a.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d %d\n", foo, bar, &foo == &bar);
  return 0;
}
EOF

cat <<EOF | cc -o $t/b.o -c -x assembler -
  .globl foo, bar
  .data;
foo:
bar:
  .long 42
EOF

clang -fuse-ld=$mold -no-pie -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '42 42 1'

echo OK
