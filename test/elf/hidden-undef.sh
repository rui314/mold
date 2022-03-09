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

cat <<EOF | $CC -o $t/a.so -shared -fPIC -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -fPIC -c -xc -
__attribute__((visibility("hidden"))) void foo();
int main() { foo(); }
EOF

! $CC -B. -o $t/exe $t/a.so $t/b.o >& $t/log
grep -q 'undefined symbol: .*b.o: foo' $t/log

echo OK
