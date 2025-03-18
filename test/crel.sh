#!/bin/bash
. $(dirname $0)/common.inc

# Currently, CREL is not supported on REL-type targets
[ $MACHINE = arm ] && skip
[ $MACHINE = i686 ] && skip

[ "$CC" = cc ] || skip
clang -c -xc -o /dev/null /dev/null -Wa,--crel,--allow-experimental-crel || skip

cat <<EOF | clang -o $t/a.o -c -xc - -Wa,--crel,--allow-experimental-crel
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep 'Hello world'
