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

cat <<EOF | $CC -o $t/foo.o -fPIC -c -xc -
__attribute__((visibility("hidden"))) void foo();
int main() { foo(); }
EOF

./mold -shared -o $t/foo.so $t/foo.o
readelf --symbols $t/foo.so > $t/log
grep -q '.*LOCAL  DEFAULT  UND foo' $t/log

echo OK
