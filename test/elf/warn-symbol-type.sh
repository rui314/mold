#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -fPIC -xc -o $t/a.o -
#include <stdio.h>
int times = -1; /* times collides with clock_t times(struct tms *buffer); */

int
main ()
{
  printf ("times: %d\n", times);
  return 0;
}
EOF

$CC -B. -shared -o $t/a.so $t/a.o >& $t/log

grep -q "warning: symbol type mismatch: times" $t/log