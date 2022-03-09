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
#include <stdio.h>

int main() {
  printf("Hello");
  fprintf(stdout, " world\n");
  fprintf(stderr, "Hello stderr\n");
}
EOF

clang -fuse-ld="$mold" -o $t/exe $t/a.o
$t/exe 2> /dev/null | grep -q 'Hello world'
$t/exe 2>&1 > /dev/null | grep -q 'Hello stderr'

echo OK
