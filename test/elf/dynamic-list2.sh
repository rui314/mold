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
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo(int x) {}
void bar(int x) {}
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
void baz(int x) {}
int main() {}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o

readelf --dyn-syms $t/exe > $t/log
! grep -q ' foo$' $t/log || false
! grep -q ' bar$' $t/log || false

cat <<EOF > $t/dyn
{ foo; extern "C++" { "baz(int)"; }; };
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-dynamic-list=$t/dyn

readelf --dyn-syms $t/exe > $t/log
grep -q ' foo$' $t/log
! grep -q ' bar$' $t/log || false
grep -q ' _Z3bazi$' $t/log

echo OK
