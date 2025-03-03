#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto || skip

cat <<EOF | $CC -flto -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -flto -o $t/exe $t/a.o -Wl,-dependency-file=$t/dep

grep '/exe:.*/a.o ' $t/dep
grep '/a.o:$' $t/dep
not grep '^/tmp' $t/dep
