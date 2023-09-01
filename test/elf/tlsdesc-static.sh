#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

if [ $MACHINE = x86_64 -o $MACHINE = i386 -o $MACHINE = arm ]; then
  dialect=gnu2
elif [ $MACHINE = aarch64 ]; then
  dialect=desc
else
  skip
fi

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;

int main() {
  foo = 42;
  printf("%d\n", foo);
}
EOF

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/b.o -xc -
_Thread_local int foo;
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o -static
$QEMU $t/exe1 | grep -q 42

$CC -B. -o $t/exe2 $t/a.o $t/b.o -static -Wl,-no-relax
$QEMU $t/exe2 | grep -q 42
