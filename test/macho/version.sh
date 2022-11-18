#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

./ld64 -v | grep -q '[ms]old'

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -Wl,-v -o $t/exe $t/a.o | grep -q '[ms]old'
$t/exe | grep -q 'Hello world'

echo OK
