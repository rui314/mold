#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-dependency-file=$t/dep

grep -q  "dependency-file/exe:.*/a.o " $t/dep
grep -q  ".*/a.o:$" $t/dep
