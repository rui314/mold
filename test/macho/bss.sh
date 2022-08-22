#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

static int foo[100];

int main() {
  foo[1] = 5;
  printf("%d %d %p\n", foo[0], foo[1], foo);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o
$t/exe | grep -q '^0 5 '

echo OK
