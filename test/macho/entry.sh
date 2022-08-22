#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int hello() {
  printf("Hello world\n");
  return 0;
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-e,_hello
$t/exe | grep -q 'Hello world'

! cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-e,no_such_symbol 2> $t/log || false
grep -q 'undefined entry point symbol: no_such_symbol' $t/log

echo OK
