#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
#include <stdio.h>
int foo() __attribute__((weak));
int main() {
  printf("%d\n", foo ? foo() : 5);
}
EOF

cat <<EOF | cc -c -o $t/b.o -xc -
int foo() { return 42; }
EOF

cc --ld-path=./ld64 -o $t/exe1 $t/a.o -Wl,-U,_foo
$t/exe1 | grep -q '^5$'

cc --ld-path=./ld64 -o $t/exe2 $t/a.o $t/b.o -Wl,-U,_foo
$t/exe2 | grep -q '^42$'

echo OK
