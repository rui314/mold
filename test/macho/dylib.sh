#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
#include <stdio.h>
char world[] = "world";

char *hello() {
  return "Hello";
}
EOF

cc --ld-path=./ld64 -o $t/b.dylib -shared $t/a.o

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

char *hello();
extern char world[];

int main() {
  printf("%s %s\n", hello(), world);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/c.o $t/b.dylib
$t/exe | grep -q 'Hello world'

echo OK
