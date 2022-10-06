#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
void foobar(void);

int main() {
  foobar();
}
EOF

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc -
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

$CC -B. -o $t/exe $t/a.o $t/b.so
$QEMU $t/exe | grep -q 'Hello world'
