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

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
__attribute__((visibility("hidden"))) void bar() {}
EOF

clang --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o
objdump --macho --exports-trie $t/b.dylib > $t/log
grep -q _foo $t/log
! grep -q _bar $t/log || false

echo OK
