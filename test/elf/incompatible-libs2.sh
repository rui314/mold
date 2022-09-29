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

cat <<EOF | $CC -m32 -c -o $t/a.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib32
$CC -m32 -shared -o $t/lib32/libfoo.so $t/a.o

cat <<EOF | $CC -c -o $t/d.o -xc -
char hello[] = "Hello world";
EOF

mkdir -p $t/lib64
$CC -shared -o $t/lib64/libfoo.so $t/d.o

cat <<EOF | $CC -c -o $t/e.o -xc -
#include <stdio.h>

extern char hello[];

int main() {
  printf("%s\n", hello);
}
EOF

mkdir -p $t/script
echo 'GROUP(libfoo.so)' > $t/script/libfoo.so

$CC -B. -o $t/exe -L$t/lib32 -L$t/lib64 -lfoo $t/e.o -Wl,-rpath $t/lib64 \
  >& $t/log

grep -q 'lib32/libfoo.so: skipping incompatible file' $t/log
$QEMU $t/exe | grep -q 'Hello world'

echo OK
