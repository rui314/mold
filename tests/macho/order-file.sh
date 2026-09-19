#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main();

void print() {
  printf("%d\n", (char *)print < (char *)main);
}

int main() {
  print();
}
EOF

cat <<EOF > $t/order1
_print
_main
EOF

cat <<EOF > $t/order2
_main
_print
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-order_file,$t/order1
$t/exe1 | grep -q '^1$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-order_file,$t/order2
$t/exe2 | grep -q '^0$'
# Arch and object-file qualifiers: lines for other arches are ignored,
# and a file qualifier restricts the match to that object's symbols.
OTHER=x86_64; [ $ARCH = x86_64 ] && OTHER=arm64
cat <<EOF > $t/order3
$OTHER:_main
$ARCH:_print
_main
EOF
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-order_file,$t/order3
$t/exe3 | grep -q '^1$'

cat <<EOF > $t/order4
nosuch.o:_main
a.o:_print
EOF
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-order_file,$t/order4
$t/exe4 | grep -q '^1$'
