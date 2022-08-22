#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

int a = 5;
int *b = &a;

void print() {
  printf("%d %d\n", a, *b);
}
EOF

cc --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o

cat <<EOF | cc -o $t/c.o -c -xc - -fPIC
void print();
int main() { print(); }
EOF

cc --ld-path=./ld64 -o $t/exe $t/b.dylib $t/c.o
$t/exe | grep -q '^5 5$'

echo OK
