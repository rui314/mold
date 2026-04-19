#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o
readelf --dyn-syms $t/b.so | grep -E '(IFUNC|<OS specific>: 10)\s+GLOBAL DEFAULT.* foobar'
