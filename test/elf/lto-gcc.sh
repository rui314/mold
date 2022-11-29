#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $GCC -flto -o /dev/null -xc - >& /dev/null \
  || skip

cat <<EOF | $GCC -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$GCC -B. -o $t/exe1 -flto $t/a.o
$QEMU $t/exe1 | grep -q 'Hello world'

# Test that LTO is used for FAT LTO objects
cat <<EOF | $GCC -flto -ffat-lto-objects -c -o $t/b.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$GCC -B. -o $t/exe2 $t/b.o --verbose 2>&1 | grep -q -- -fwpa
