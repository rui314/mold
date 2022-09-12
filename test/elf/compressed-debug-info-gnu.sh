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

which dwarfdump >& /dev/null || { echo skipped; exit; }

# Clang may not support -gz=zlib-gnu
echo 'int main() {}' | $CXX -o /dev/null -gz=zlib-gnu -xc++ ->& /dev/null ||
  { echo skipped; exit; }

cat <<EOF | $CXX -c -o $t/a.o -g -gz=zlib-gnu -xc++ -
int main() {
  return 0;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -g -gz=zlib-gnu -xc++ -
int foo() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
dwarfdump $t/exe > /dev/null
readelf --sections $t/exe | grep -Fq .debug_info

echo ' OK'
