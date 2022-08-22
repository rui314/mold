#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

lipo $t/a.o -create -output $t/fat.o

cat <<EOF | cc -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
