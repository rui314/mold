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

! cc --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o -Wl,-pagezero_size,0x1000 >& $t/log
grep -Fq ' -pagezero_size option can only be used when linking a main executable' $t/log

echo OK
