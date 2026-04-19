#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -fPIC -xc -o $t/a.o -
#include <stdio.h>
int times = -1; // times collides with clock_t times(struct tms *buffer)
int main() {
  printf ("times: %d\n", times);
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o |& grep 'warning: symbol type mismatch: times'
