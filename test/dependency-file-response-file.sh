#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

echo "$t/a.o -Wl,-dependency-file=$t/dep" > $t/rsp

$CC -B. -o $t/exe @$t/rsp

grep '/exe:.*/a.o ' $t/dep
grep '/a.o:$' $t/dep
not grep '^/tmp' $t/dep
