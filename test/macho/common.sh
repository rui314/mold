#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -fcommon -c -xc -
int foo;
int bar;
EOF

cat <<EOF | cc -o $t/b.o -fcommon -c -xc -
int foo;
int bar = 5;
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;
static int baz[10000];

int main() {
  printf("%d %d %d\n", foo, bar, baz[0]);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^0 5 0$'

echo OK
