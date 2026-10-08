#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,--print-dependencies > $t/log

grep -Eq '/a\.o\s.*libSystem\S+\su\s_printf' $t/log
grep -Eq '/b\.o\s.*a\.o\su\s_hello' $t/log
