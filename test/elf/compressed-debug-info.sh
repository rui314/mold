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

which dwarfdump >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CXX -c -o $t/a.o -g -gz=zlib-gnu -xc++ -
int main() {
  return 0;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -g -gz=zlib -xc++ -
int foo() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
dwarfdump $t/exe > /dev/null
readelf --sections $t/exe | fgrep -q .debug_info

echo ' OK'
