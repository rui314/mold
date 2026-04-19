#!/bin/bash
. $(dirname $0)/common.inc

command -v $GXX >& /dev/null || skip

cat <<EOF | $GXX -o $t/a.o -c -std=c++17 -fno-gnu-unique -xc++ -
inline int foo = 5;
int bar() { return foo; }
EOF

cat <<EOF | $GXX -o $t/b.o -c -std=c++17 -fgnu-unique -xc++ -
#include <stdio.h>

inline int foo = 5;

int main() {
  printf("foo=%d\n", foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -no-pie
$QEMU $t/exe | grep 'foo=5'
