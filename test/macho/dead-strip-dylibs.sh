#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o -L$t -Wl,-lfoo
otool -l $t/exe | grep -A3 LOAD_DY | grep -q libfoo.dylib

clang -fuse-ld="$mold" -o $t/exe $t/a.o -L$t -Wl,-lfoo -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 LOAD_DY > $t/log
! grep -q libfoo.dylib $t/log || false

echo OK
