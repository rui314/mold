#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = m68k ] && skip
[ $MACHINE = sh4 ] && skip
[ $MACHINE = sh4aeb ] && skip

cat <<EOF | $CXX -c -o $t/a.o -xc++ - -ffunction-sections
int foo() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -xc++ - -ffunction-sections
#include <iostream>
int foo();
int main() { std::cout << foo() << "\n"; }
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o
$CXX -B. -o $t/exe $t/c.o -Wl,--gc-sections
$QEMU $t/exe | grep '^0$'
