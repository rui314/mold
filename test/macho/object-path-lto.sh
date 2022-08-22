#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc - -flto
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -flto -Wl,-object_path_lto,$t/obj
$t/exe | grep -q 'Hello world'
otool -l $t/obj > /dev/null

echo OK
