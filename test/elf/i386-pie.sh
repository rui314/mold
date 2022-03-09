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

cat <<EOF | $CC -m32 -o $t/a.o -c -xc - -fPIC
char msg[] = "Hello world";
EOF

cat <<EOF | $CC -m32 -o $t/b.o -c -xc - -fPIC
#include <stdio.h>

extern char msg[];

int main() {
  printf("%s\n", msg);
  return 0;
}
EOF

$CC -B. -m32 -o $t/exe1 $t/a.o $t/b.o -pie
$t/exe1 | grep -q 'Hello world'

$CC -B. -m32 -o $t/exe2 $t/a.o $t/b.o -no-pie
$t/exe2 | grep -q 'Hello world'

echo OK
