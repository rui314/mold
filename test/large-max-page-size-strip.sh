#!/bin/bash
. $(dirname $0)/common.inc

# ARM32's strip command crashes on the output of this test for some reason.
[[ $MACHINE = arm* ]] && skip

strip=$STRIP
command -v $strip >& /dev/null || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -pie -Wl,-zmax-page-size=0x200000
$strip $t/exe
$QEMU $t/exe | grep 'Hello world'
