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

cat <<EOF | $CC -o $t/a.o -c -xc -
char msg[] = "Hello world\n";
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

extern char msg[];

int main() {
  printf("%s\n", msg);
}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
