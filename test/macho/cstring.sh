#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
const char *x = "Hello world\n";
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

extern const char *x;
const char *y = "Hello world\n";
const char *z = "Howdy world\n";

int main() {
  printf("%d %d\n", x == y, y == z);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '^1 0$'

echo OK
