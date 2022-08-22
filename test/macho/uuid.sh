#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe1 $t/a.o -Wl,-final_output,exe1
otool -l $t/exe1 | grep -q LC_UUID

cc --ld-path=./ld64 -o $t/exe2 $t/a.o -Wl,-final_output,exe1
otool -l $t/exe2 | grep -q LC_UUID

diff -q $t/exe1 $t/exe2 > /dev/null

cc --ld-path=./ld64 -o $t/exe3 $t/a.o -Wl,-no_uuid
otool -l $t/exe3 > $t/log3
! grep -q LC_UUID $t/log3 || false

cc --ld-path=./ld64 -o $t/exe4 $t/a.o -Wl,-random_uuid
otool -l $t/exe4 | grep -q LC_UUID

echo OK
