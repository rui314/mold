#!/bin/bash
. $(dirname $0)/common.inc

if [ $MACHINE = x86_64 ]; then
  dialect=gnu2
elif [ $MACHINE = aarch64 ]; then
  dialect=desc
else
  skip
fi

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;
extern _Thread_local int bar;

int main() {
  bar = 7;
  printf("%d %d\n", foo, bar);
}
EOF

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -shared -o $t/b.so -xc -
_Thread_local int foo = 5;
_Thread_local int bar;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
$QEMU $t/exe | grep -q '5 7'
