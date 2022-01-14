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

which llvm-readelf >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -pie
llvm-readelf -r $t/exe | grep RELATIVE | cut -d' ' -f1 | sort > $t/log1

$CC -B. -o $t/exe $t/a.o -pie -Wl,-pack-dyn-relocs=relr
llvm-readelf -r $t/exe | grep RELATIVE | cut -d' ' -f1 | sort > $t/log2

diff $t/log1 $t/log2

echo OK
