#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -B. -o $t/exe1 $t/a.o -Wl,-adhoc_codesign
cc --ld-path=./ld64 -B. -o $t/exe2 $t/a.o -Wl,-adhoc_codesign

[ "$(otool -l $t/exe1 | grep 'uuid ')" != "$(otool -l $t/exe2 | grep 'uuid ')" ]

echo OK
