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
void expfn1() {}
void expfn2() {}
void foo();

int main() {
  expfn1();
  expfn2();
  foo();
}
EOF

cat <<EOF | $CC -shared -o $t/b.so -xc -
void expfn1();
void expfn2() {}

void foo() {
  expfn1();
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
readelf --dyn-syms $t/exe | grep -q expfn2
readelf --dyn-syms $t/exe | grep -q expfn1

echo OK
