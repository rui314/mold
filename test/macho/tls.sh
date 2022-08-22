#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -shared -o $t/a.dylib -xc -
_Thread_local int b;
_Thread_local int c = 5;
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

int a = 3;
extern _Thread_local int b;
extern _Thread_local int c;

int main() {
  printf("%d %d %d\n", a, b, c);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.dylib $t/b.o
$t/exe | grep -q '^3 0 5$'

echo OK
