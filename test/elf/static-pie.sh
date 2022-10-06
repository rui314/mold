#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

# We need to implement R_386_GOT32X relaxation to support PIE on i386
[ $MACHINE = i386 ] && skip

[ $MACHINE = aarch64 ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

# Skip if the system does not support -static-pie
$CC -o $t/exe1 $t/a.o -static-pie >& /dev/null || skip
$QEMU $t/exe1 >& /dev/null || skip

$CC -B. -o $t/exe2 $t/a.o -static-pie
$QEMU $t/exe2 | grep -q 'Hello world'
