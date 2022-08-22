#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

mkdir -p $t/Foo.framework

cat <<EOF | cc -o $t/Foo.framework/Foo -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | cc -o $t/a.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-F$t -Wl,-framework,Foo
$t/exe | grep -q 'Hello world'

echo OK
