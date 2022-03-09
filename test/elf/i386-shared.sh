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

echo 'int main() {}' | $CC -m32 -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc - -m32
int foo = 5;
void set_foo(int x) { foo = x; }
int get_foo() { return foo; }

static int bar = 7;
void set_bar(int x) { bar = x; }
int get_bar() { return bar; }
EOF

$CC -B. -o $t/b.so -shared $t/a.o -m32

cat <<EOF > $t/c.c
#include <stdio.h>

int get_foo();
int get_bar();

int baz = 2;

int main() {
  printf("%d %d %d\n", get_foo(), get_bar(), baz);
}
EOF

$CC -c -o $t/d.o $t/c.c -fno-PIC -m32
$CC -B. -o $t/exe $t/d.o $t/b.so -m32 -no-pie
$t/exe | grep -q '5 7 2'

$CC -c -o $t/e.o $t/c.c -fPIE -m32
$CC -B. -o $t/exe $t/e.o $t/b.so -m32 -pie
$t/exe | grep -q '5 7 2'

$CC -c -o $t/f.o $t/c.c -fPIC -m32
$CC -B. -o $t/exe $t/f.o $t/b.so -m32 -pie
$t/exe | grep -q '5 7 2'

echo OK
