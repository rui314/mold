#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-package-metadata='{"foo":"bar"}'
readelf -x .note.package $t/exe | grep -Fq '{"foo":"bar"}'
