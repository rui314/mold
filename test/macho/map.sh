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

cat <<EOF | cc -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o -Wl,-map,$t/map

grep -Eq '^\[  0\] .*/a.o$' $t/map
grep -Eq '^\[  1\] .*/b.o$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      __TEXT  __text$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      \[  0\] _hello$' $t/map
grep -Eq '^0x[0-9A-Fa-f]+     0x[0-9A-Fa-f]+      \[  1\] _main$' $t/map

echo OK
