#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
int foo() { return 42; }
EOF

cat <<EOF | cc --ld-path=./ld64 -undefined dynamic_lookup -shared -o $t/b.dylib -xc -
int foo();
int bar() { return foo(); }
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

int bar();
int main() {
  printf("%d\n", bar());
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.dylib $t/c.o
$t/exe | grep -q '42'

echo OK
