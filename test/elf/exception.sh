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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

static=1
echo 'int main() {}' | $CC -o /dev/null -xc - -static >& /dev/null || static=0

cat <<EOF > $t/a.cc
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

$CXX -c -o $t/b.o $t/a.cc -fPIC
$CXX -c -o $t/c.o $t/a.cc -fno-PIC

if [ $static = 1 ]; then
  $CXX -B. -o $t/exe1 $t/b.o -static
  $QEMU $t/exe1
fi

if [ $static = 1 ]; then
  $CXX -B. -o $t/exe2 $t/c.o -static
  $QEMU $t/exe2
fi

$CXX -B. -o $t/exe3 $t/b.o -pie
$QEMU $t/exe3

$CXX -B. -o $t/exe4 $t/c.o -no-pie
$QEMU $t/exe4

$CXX -B. -o $t/exe5 $t/b.o -pie -Wl,--gc-sections
$QEMU $t/exe5

if [ $static = 1 ]; then
  $CXX -B. -o $t/exe6 $t/c.o -static -Wl,--gc-sections
  $QEMU $t/exe6
fi

if [ $MACHINE = x86_64 ]; then
  $CXX -c -o $t/d.o $t/a.cc -mcmodel=large -fPIC

  if [ $static = 1 ]; then
    $CXX -B. -o $t/exe7 $t/d.o -static
    $QEMU $t/exe7
  fi

  $CXX -B. -o $t/exe8 $t/d.o -pie
  $QEMU $t/exe8
fi

if [ $MACHINE = x86_64 -o $MACHINE = aarch64 ]; then
  $CXX -c -o $t/e.o $t/a.cc -mcmodel=large -fno-PIC

  if [ $static = 1 ]; then
    $CXX -B. -o $t/exe9 $t/e.o -static
    $QEMU $t/exe9
  fi

  $CXX -B. -o $t/exe10 $t/e.o -no-pie
  $QEMU $t/exe10
fi

echo OK
