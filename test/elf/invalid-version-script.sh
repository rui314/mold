#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

echo 'int main() {}' | $CC -c -o $t/a.o -xc -

echo 'VER1 { foo[12; };' > $t/b.ver

! $CC -B. -shared -o $t/c.so -Wl,-version-script,$t/b.ver \
  $t/a.o >& $t/log || false
grep -q 'invalid version pattern' $t/log

echo OK
