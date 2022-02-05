#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -c -xc -o $t/a.o -
extern int foo;
int x() { return foo; }
EOF

cat <<EOF | $CC -c -xc -o $t/b.o -
extern int foo;
int y() { return foo; }
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--warn-unresolved-symbols >& $t/log1
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--warn-unresolved-symbols,--warn-once >& $t/log2

[ "$(grep 'undefined symbol:.* foo$' $t/log1 | wc -l)" = 2 ]
[ "$(grep 'undefined symbol:.* foo$' $t/log2 | wc -l)" = 1 ]

echo OK
