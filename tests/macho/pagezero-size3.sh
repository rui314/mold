#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o
otool -l $t/b.dylib > $t/log
! grep -q 'segname: __PAGEZERO' $t/log || false
