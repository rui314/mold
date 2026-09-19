#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

lipo $t/a.o -create -output $t/fat.o

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'
