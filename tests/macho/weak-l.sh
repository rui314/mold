#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() __attribute__((weak_import));

int main() {
  if (hello)
    hello();
  else
    printf("hello is missing\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-weak-lfoo
$RUN $t/exe | grep 'Hello world'

rm $t/libfoo.dylib
$RUN $t/exe | grep 'hello is missing'
