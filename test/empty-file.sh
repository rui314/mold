#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

rm -f $t/b.script
touch $t/b.script

$CC -B. -o $t/exe $t/a.o -Wl,--version-script,$t/b.script
$QEMU $t/exe | grep 'Hello world'
