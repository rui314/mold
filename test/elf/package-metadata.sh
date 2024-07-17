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

$CC -B. -o $t/exe2 $t/a.o -Wl,--encoded-package-metadata=%7B%22foo%22%3A%22bar%22%7D
readelf -x .note.package $t/exe2 | grep -Fq '{"foo":"bar"}'
