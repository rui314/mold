#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -flto -Wl,-object_path_lto,$t/obj
$t/exe | grep -q 'Hello world'
otool -l $t/obj > /dev/null
