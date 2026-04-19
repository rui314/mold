#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
#include <stdio.h>

void foo() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,interpose
readelf --dynamic $t/b.so | grep 'Flags:.*INTERPOSE'
