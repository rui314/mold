#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep 'Hello world'
