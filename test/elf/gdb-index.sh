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

cat <<EOF > $t/a.c
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}

void greet() {
  hello();
}
EOF

$CC -o $t/b.o -c -ggnu-pubnames -g $t/a.c
$CC -o $t/c.o -c -ggnu-pubnames -g $t/a.c -gz

cat <<EOF | $CC -o $t/d.o -c -xc -ggnu-pubnames -g -
void greet();

int main() {
  greet();
}
EOF

$CC -B. -o $t/exe1 $t/b.o $t/d.o -Wl,--gdb-index
$QEMU $t/exe1 | grep -q 'Hello world'
readelf -WS $t/exe1 | fgrep -q .gdb_index
DEBUGINFOD_URLS= gdb $t/exe1 -ex 'b main' -ex run -ex cont -ex quit >& /dev/null

$CC -B. -o $t/exe2 $t/c.o $t/d.o -Wl,--gdb-index
$QEMU $t/exe2 | grep -q 'Hello world'
readelf -WS $t/exe2 | fgrep -q .gdb_index
DEBUGINFOD_URLS= gdb $t/exe2 -ex 'b main' -ex run -ex cont -ex quit >& /dev/null

echo OK
