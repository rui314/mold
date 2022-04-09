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

echo 'int main() {}' | $CC -o $t/exe -xc -
dyld=$(readelf -Wl $t/exe | grep 'program interpreter:' | sed -e 's!.*: \(.*\)\]!\1!')

cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
#include <stdio.h>
#include <stdlib.h>

void hello() {
  puts("Hello world");
}

void _start() {
  hello();
  exit(0);
}
EOF

$CC -B. -o $t/b.so $t/a.o -shared -Wl,--dynamic-linker=$dyld
$QEMU $t/b.so | grep -q 'Hello world'

cat <<EOF | $CC -o $t/c.o -c -xc -
void hello();
int main() { hello(); }
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q 'Hello world'

echo OK
