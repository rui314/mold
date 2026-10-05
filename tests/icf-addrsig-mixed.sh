#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Clang emits an address-significance table, which lists no symbol for
# a.o because a.c doesn't use the addresses of x and y. b.o has no table,
# and b.c compares their addresses, so ICF must not merge them.
cat <<EOF | clang ${TRIPLE:+--target=$TRIPLE} -c -o $t/a.o -fdata-sections -xc - || skip
const int x[4] = {1, 2, 3, 4};
const int y[4] = {1, 2, 3, 4};
EOF
readelf -S $t/a.o | grep -q llvm_addrsig || skip

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

extern const int x[4], y[4];

int main() {
  printf("%d\n", x == y);
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -Wl,--icf=safe
$QEMU $t/exe1 | grep '^0$'

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--icf=all
$QEMU $t/exe2 | grep '^0$'
