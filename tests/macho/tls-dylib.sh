#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
_Thread_local int counter = 5;
int bump() { return ++counter; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o \
  -install_name $PWD/$t/libfoo.dylib

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
extern _Thread_local int counter;
int bump();
int main() {
  printf("%d %d %d\n", counter, bump(), counter);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o $t/libfoo.dylib
$t/exe | grep '^5 6 6$'
