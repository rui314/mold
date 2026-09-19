#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
extern int x;
int get_x() { return x; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int x = 7;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int get_x();
int main() {
  printf("%d\n", get_x());
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep '^7$'
