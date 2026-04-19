#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
void foobar(void);

int main() {
  foobar();
}
EOF

cat <<EOF | $CC -fPIC -o $t/b.o -c -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

$CC -B. -o $t/c.so $t/b.o -shared
readelf -W --dyn-syms $t/c.so | grep -E '(IFUNC|<OS specific>: 10).*foobar'

$CC -B. -o $t/exe $t/a.o $t/c.so
$QEMU $t/exe | grep 'Hello world'
