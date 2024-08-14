#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -shared -o $t/a.so -xc -
void fn() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -fno-PIC -
#include <stdio.h>

typedef void Func();

void fn();
Func *const ptr = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

$CC -B. -o $t/exe -no-pie $t/b.o $t/a.so
$QEMU $t/exe | grep -q 1
