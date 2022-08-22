#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
void hello() __attribute__((weak_import));

int main() {
  if (hello)
    hello();
  else
    printf("hello is missing\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -L$t -Wl,-weak-lfoo
$t/exe | grep -q 'Hello world'

rm $t/libfoo.dylib
$t/exe | grep -q 'hello is missing'

echo OK
