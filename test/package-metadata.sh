#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-package-metadata='{"foo":"bar"}'
readelf -x .note.package $t/exe1 | grep -Fq '{"foo":"bar"}'

$CC -B. -o $t/exe2 $t/a.o -Wl,-package-metadata=%7B%22foo%22%3A%22bar%22%7D
readelf -x .note.package $t/exe2 | grep -Fq '{"foo":"bar"}'

$CC -B. -o $t/exe3 $t/a.o -Wl,-package-metadata="%[lbrace]%[quot]foo%[quot]:%[quot]bar%[quot]%[rbrace]"
readelf -x .note.package $t/exe3 | grep -Fq '{"foo":"bar"}'

$CC -B. -o $t/exe4 $t/a.o -Wl,-package-metadata=%7B%22name%22:%22mold%22%2C%22ver%22%3A%22x%20%%22%7d
readelf -p .note.package $t/exe4 | grep -Fq '{"name":"mold","ver":"x %"}'

$CC -B. -o $t/exe5 $t/a.o -Wl,-package-metadata="{%[quot]name%[quot]:%[quot]mold%[quot]%[comma]%[quot]ver%[quot]:%[quot]x%[space]%%[quot]}"
readelf -p .note.package $t/exe5 | grep -Fq '{"name":"mold","ver":"x %"}'
