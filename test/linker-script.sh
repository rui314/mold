#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cat <<EOF > $t/script
GROUP("$t/a.o")
EOF

$CC -B. -o $t/exe $t/script
$QEMU $t/exe | grep 'Hello world'

$CC -B. -o $t/exe -Wl,-T,$t/script
$QEMU $t/exe | grep 'Hello world'

$CC -B. -o $t/exe -Wl,--script,$t/script
$QEMU $t/exe | grep 'Hello world'
