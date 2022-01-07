#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

echo 'int main() {}' | $CC -m32 -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -m64 -xc -
int main() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -m32 -xc -
EOF

! $CC -B. -o /dev/null $t/a.o $t/b.o >& $t/log
grep -q "$t/b.o: incompatible file" $t/log

echo OK
