#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int three() { return 3; }
int value = 9;
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o \
  -install_name $PWD/$t/libfoo.dylib

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int three();
extern int value;
int main() {
  printf("%d %d\n", three(), value);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o $t/libfoo.dylib
$t/exe | grep '3 9'
