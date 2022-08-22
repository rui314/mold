#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

[ "`uname -p`" = arm ] && { echo skipped; exit; }

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o

otool -l $t/exe | grep -A5 'segname __PAGEZERO' | \
  grep -q 'vmsize 0x0000000100000000'

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-pagezero_size,0x10000
$t/exe | grep -q 'Hello world'

otool -l $t/exe | grep -A5 'segname __PAGEZERO' | \
  grep -q 'vmsize 0x0000000000010000'

echo OK
