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

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
void bar() {}
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o

readelf --dyn-syms $t/exe > $t/log
! grep -q ' foo$' $t/log || false
! grep -q ' bar$' $t/log || false

cat <<EOF > $t/dyn
{ foo; bar; };
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-dynamic-list=$t/dyn

readelf --dyn-syms $t/exe > $t/log
grep -q ' foo$' $t/log
grep -q ' bar$' $t/log

echo OK
