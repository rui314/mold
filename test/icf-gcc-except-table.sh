#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = arm ] && skip
[[ $MACHINE = riscv* ]] && skip
[[ $MACHINE = loongarch* ]] && skip

cat <<EOF | $CXX -c -o $t/a.o -ffunction-sections -fdata-sections -xc++ -
#include <stdexcept>

template <typename T>
struct X {
  static void raise() { throw std::logic_error("foo"); }
};

int main() {
  X<int>().raise();
  X<float>().raise();
}
EOF

$CXX -B. -o $t/exe $t/a.o -Wl,-icf=safe,--print-icf-sections |&
  grep -E 'removing .*/a.o:\(.gcc_except_table'
