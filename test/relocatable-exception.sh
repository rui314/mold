#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = m68k ] && skip
[ $MACHINE = sh4 ] && skip
[ $MACHINE = sh4aeb ] && skip

# OneTBB isn't tsan-clean
nm mold | grep -q '__tsan_init' && skip

cat <<EOF | $CXX -c -o $t/a.o -xc++ -
int foo() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -xc++ -
#include <iostream>
int foo();
int main() { std::cout << foo() << "\n"; }
EOF

./mold --relocatable -o $t/c.o $t/a.o $t/b.o

$CXX -B. -o $t/exe $t/c.o
$QEMU $t/exe
