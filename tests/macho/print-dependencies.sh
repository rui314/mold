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

# Tab characters spelled out: GNU grep 3.8+ treats \t in a pattern as
# a stray backslash.
grep -Eq $'/a\\.o\t.*libSystem\\S+\tu\t_printf' $t/log
grep -Eq $'/b\\.o\t.*a\\.o\tu\t_hello' $t/log
