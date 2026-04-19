#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.c
#include <stdio.h>

__attribute__((tls_model("global-dynamic"))) static _Thread_local int x1 = 1;
__attribute__((tls_model("global-dynamic"))) static _Thread_local int x2;
__attribute__((tls_model("global-dynamic"))) extern _Thread_local int x3;
__attribute__((tls_model("global-dynamic"))) extern _Thread_local int x4;

int get_x5();
int get_x6();

int main() {
  x2 = 2;

  printf("%d %d %d %d %d %d\n", x1, x2, x3, x4, get_x5(), get_x6());
  return 0;
}
EOF

$CC -fPIC -c -o $t/b.o $t/a.c
$CC -fPIC -c -o $t/c.o $t/a.c -O2

cat <<EOF | $CC -fPIC -c -o $t/d.o -xc -
__attribute__((tls_model("global-dynamic"))) _Thread_local int x3 = 3;
__attribute__((tls_model("global-dynamic"))) static _Thread_local int x5 = 5;
int get_x5() { return x5; }
EOF

cat <<EOF | $CC -fPIC -c -o $t/e.o -xc -
__attribute__((tls_model("global-dynamic"))) _Thread_local int x4 = 4;
__attribute__((tls_model("global-dynamic"))) static _Thread_local int x6 = 6;
int get_x6() { return x6; }
EOF

$CC -B. -shared -o $t/f.so $t/d.o
$CC -B. -shared -o $t/g.so $t/e.o -Wl,--no-relax

$CC -B. -o $t/exe1 $t/b.o $t/f.so $t/g.so
$QEMU $t/exe1 | grep '1 2 3 4 5 6'

$CC -B. -o $t/exe2 $t/c.o $t/f.so $t/g.so
$QEMU $t/exe2 | grep '1 2 3 4 5 6'

$CC -B. -o $t/exe3 $t/b.o $t/f.so $t/g.so -Wl,-no-relax
$QEMU $t/exe3 | grep '1 2 3 4 5 6'

$CC -B. -o $t/exe4 $t/c.o $t/f.so $t/g.so -Wl,-no-relax
$QEMU $t/exe4 | grep '1 2 3 4 5 6'

if test_cflags -static; then
  $CC -B. -o $t/exe5 $t/b.o $t/d.o $t/e.o -static
  $QEMU $t/exe5 | grep '1 2 3 4 5 6'

  $CC -B. -o $t/exe6 $t/c.o $t/d.o $t/e.o -static
  $QEMU $t/exe6 | grep '1 2 3 4 5 6'

  $CC -B. -o $t/exe7 $t/b.o $t/d.o $t/e.o -static -Wl,-no-relax
  $QEMU $t/exe7 | grep '1 2 3 4 5 6'

  $CC -B. -o $t/exe8 $t/c.o $t/d.o $t/e.o -static -Wl,-no-relax
  $QEMU $t/exe8 | grep '1 2 3 4 5 6'
fi
