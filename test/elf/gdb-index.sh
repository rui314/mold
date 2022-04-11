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

[ $MACHINE = $(uname -m) ] || { echo skipped; exit; }

which gdb >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -ggnu-pubnames -g - -ffunction-sections
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}

void greet() {
  hello();
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -ggnu-pubnames -g -
void greet();

int main() {
  greet();
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--gdb-index
$QEMU $t/exe | grep -q 'Hello world'

readelf -WS $t/exe | fgrep -q .gdb_index

gdb $t/exe -ex 'b main' -ex run -ex cont -ex quit >& /dev/null

echo OK
