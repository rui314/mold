#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello");
  fprintf(stdout, " world\n");
  fprintf(stderr, "Hello stderr\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o
$t/exe 2> /dev/null | grep -q 'Hello world'
$t/exe 2>&1 > /dev/null | grep -q 'Hello stderr'

echo OK
