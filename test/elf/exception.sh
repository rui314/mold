#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CXX -c -o $t/a.o -xc++ -fPIC -
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

$CXX -B. -o $t/exe $t/a.o -static
$QEMU $t/exe

$CXX -B. -o $t/exe $t/a.o
$QEMU $t/exe

$CXX -B. -o $t/exe $t/a.o -Wl,--gc-sections
$QEMU $t/exe

$CXX -B. -o $t/exe $t/a.o -static -Wl,--gc-sections
$QEMU $t/exe

if [ $MACHINE = x86_64 -o $MACHINE = aarch64 ]; then
  $CXX -B. -o $t/exe $t/a.o -mcmodel=large -fno-PIC
  $QEMU $t/exe
fi

echo OK
