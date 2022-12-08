#!/bin/bash
. $(dirname $0)/common.inc

echo 'int main() {}' | $CC -flto -o /dev/null -xc - >& /dev/null \
  || skip

cat <<EOF | $CC -flto -fPIC -o $t/a.o -c -xc -
#include <stdio.h>

extern void live_func();
void dead_func() {
  printf("OK\n");
}

int main() {
  live_func();
}
EOF

$CC -B. -flto -o $t/exe $t/a.o -Wl,-defsym,live_func=dead_func

$QEMU $t/exe | grep -q "^OK$"
