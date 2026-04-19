#!/bin/bash
. $(dirname $0)/common.inc

[[ $MACHINE = ppc* ]] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
void hello();
int main() { hello(); }
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
readelf -W --syms $t/exe | not grep -F 'hello$thunk'
