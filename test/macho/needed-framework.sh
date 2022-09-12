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
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-F$t -Wl,-needed_framework,Foo \
  -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' | grep -Fq Foo.framework/Foo

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-F$t -Wl,-framework,Foo \
  -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' >& $t/log
! grep -Fq Foo.framework/Foo $t/log || false

echo OK
