#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

echo "INPUT($t/a.o)" > $t/script
echo '{ global: main; local: *; };' > $t/version

$CC -B. -o $t/exe $t/script -Wl,--version-script=$t/version -Wl,-dependency-file=$t/dep

grep  "dependency-file/exe:.*/a.o " $t/dep
grep  ".*/a.o:$" $t/dep
grep  ".*/script " $t/dep
grep  ".*/script:$" $t/dep
grep  ".*/version " $t/dep
grep  ".*/version:$" $t/dep
