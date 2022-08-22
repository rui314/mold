#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
char msg[] = "Hello world\n";
EOF

cat <<EOF | cc -o $t/b.o -c -xc -
#include <stdio.h>

extern char msg[];

int main() {
  printf("%s\n", msg);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
