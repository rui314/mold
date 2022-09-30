#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

echo 'int main() {}' | $CC -m32 -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -m64 -xc -
int main() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -m32 -xc -
EOF

! $CC -B. -o /dev/null $t/a.o $t/b.o >& $t/log
grep -q "$t/b.o: incompatible file type: x86_64 is expected but got i386" $t/log

echo OK
