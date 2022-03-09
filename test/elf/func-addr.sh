#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -shared -o $t/a.so -xc -
void fn() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -fno-PIC -
#include <stdio.h>

typedef void Func();

void fn();
Func *const ptr = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

$CC -B. -o $t/exe -no-pie $t/b.o $t/a.so
$t/exe | grep -q 1

echo OK
