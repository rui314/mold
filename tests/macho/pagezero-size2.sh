#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

not $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o -Wl,-pagezero_size,0x1000 >& $t/log
grep -Fq ' -pagezero_size option can only be used when linking a main executable' $t/log
