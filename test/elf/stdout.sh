#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -Wl,-build-id=sha1 $t/a.o -o - > $t/exe
chmod 755 $t/exe
$QEMU $t/exe | grep -q 'Hello world'
