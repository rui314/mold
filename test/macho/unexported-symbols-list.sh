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
void baz() {}
void abc() {}
void xyz() {}
EOF

cat <<EOF > $t/list
_foo
_a*
EOF

clang --ld-path=./ld64 -shared -o $t/c.dylib $t/a.o

objdump --macho --exports-trie $t/c.dylib > $t/log1
grep -q _foo $t/log1
! grep -q _bar $t/log1 || false
grep -q _baz $t/log1
grep -q _abc $t/log1
grep -q _xyz $t/log1

clang --ld-path=./ld64 -shared -o $t/d.dylib $t/a.o \
  -Wl,-unexported_symbols_list,$t/list

objdump --macho --exports-trie $t/d.dylib > $t/log2
! grep -q _foo $t/log2 || false
! grep -q _bar $t/log2 || false
grep -q _baz $t/log2 || false
! grep -q _abc $t/log2 || false
grep -q _xyz $t/log2

clang --ld-path=./ld64 -shared -o $t/e.dylib $t/a.o -Wl,-unexported_symbol,_foo

objdump --macho --exports-trie $t/e.dylib > $t/log3
! grep -q _foo $t/log3 || false
! grep -q _bar $t/log3 || false
grep -q _baz $t/log3
grep -q _abc $t/log3
grep -q _xyz $t/log3

echo OK
