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

cat <<EOF | $CC -c -fPIC -xc -o $t/a.o -
extern int foo;
int x() { return foo; }
EOF

cat <<EOF | $CC -c -fPIC -xc -o $t/b.o -
extern int foo;
int y() { return foo; }
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--warn-unresolved-symbols,--warn-once >& $t/log

[ "$(grep 'undefined symbol:.* foo$' $t/log | wc -l)" = 1 ]

echo OK
