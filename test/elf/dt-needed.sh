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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/libfoo.so $t/a.o -Wl,--soname,libfoo
$CC -B. -shared -o $t/libbar.so $t/a.o

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/b.o $t/libfoo.so
readelf --dynamic $t/exe | grep -Fq 'Shared library: [libfoo]'

$CC -B. -o $t/exe $t/b.o -L $t -lfoo
readelf --dynamic $t/exe | grep -Fq 'Shared library: [libfoo]'

$CC -B. -o $t/exe $t/b.o $t/libbar.so
readelf --dynamic $t/exe | grep -Eq 'Shared library: \[.*dt-needed/libbar\.so\]'

$CC -B. -o $t/exe $t/b.o -L$t -lbar
readelf --dynamic $t/exe | grep -Fq 'Shared library: [libbar.so]'

echo OK
