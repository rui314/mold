#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -lfoo
$t/exe
