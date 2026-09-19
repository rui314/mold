#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

char arr[100000];

int main() {
  printf("%d %d\n", arr[0], arr[99999]);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep '0 0'
