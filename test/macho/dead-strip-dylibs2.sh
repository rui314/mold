#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

mkdir -p $t/Foo.framework

cat <<EOF | $CC -o $t/Foo.framework/Foo -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-F$t -Wl,-framework,Foo
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' | fgrep -q Foo.framework/Foo

clang --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-F$t -Wl,-framework,Foo \
  -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' >& $t/log
! fgrep -q Foo.framework/Foo $t/log || false

echo OK
