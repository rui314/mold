#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  puts("Hello world");
}
EOF

$CC -B. -o $t/exe $t/a.o
nm $t/exe | grep 'U puts$'
