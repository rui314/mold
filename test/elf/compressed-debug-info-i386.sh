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

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

which dwarfdump >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CXX -c -o $t/a.o -g -gz=zlib-gnu -xc++ - -m32
int main() {
  return 0;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -g -gz=zlib -xc++ - -m32
int foo() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -m32
dwarfdump $t/exe > /dev/null
readelf --sections $t/exe | fgrep -q .debug_info

echo ' OK'
