#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A lazily-bound PLT entry must preserve the pointer to the buffer for a
# struct return value, which some psABIs pass in a register.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
struct Big { int x[8]; };

struct Big make(int v) {
  struct Big b;
  for (int i = 0; i < 8; i++)
    b.x[i] = v + i;
  return b;
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
struct Big { int x[8]; };
struct Big make(int v);
int get(int v) { return make(v).x[7]; }
EOF

$CC -B. -shared -o $t/d.so $t/c.o $t/b.so -Wl,-z,lazy

cat <<EOF | $CC -c -o $t/e.o -fno-PIE -xc -
#include <stdio.h>
struct Big { int x[8]; };
struct Big make(int v);
int get(int v);
int main() { printf("%d %d\n", make(10).x[7], get(20)); }
EOF

$CC -B. -no-pie -o $t/exe $t/e.o $t/d.so $t/b.so -Wl,-z,lazy -Wl,-rpath=$t
$QEMU $t/exe | grep '^17 27$'
