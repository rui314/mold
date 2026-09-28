#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-dependency-file=$t/dep

grep -E "dependency-file/exe:.*/a.o( |$)" $t/dep
grep  ".*/a.o:$" $t/dep

# Input files are opened in parallel, but the output must not depend on
# the order in which they were opened.
$CC -B. -o $t/exe $t/a.o -Wl,-dependency-file=$t/dep2
cmp $t/dep $t/dep2
