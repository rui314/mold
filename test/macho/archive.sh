#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | cc -c -o $t/b.o -xc -
void foo() {}
EOF

rm -f $t/c.a
ar rcs $t/c.a $t/a.o $t/b.o

cat <<EOF | cc -c -o $t/d.o -xc -
void hello();

int main() {
  hello();
}
EOF

c++ --ld-path=./ld64 -o $t/exe $t/d.o $t/c.a
$t/exe | grep -q 'Hello world'

otool -tv $t/exe | grep -q '^_hello:'
! otool -tv $t/exe | grep -q '^_foo:' || false

echo OK
