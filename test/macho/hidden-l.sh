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

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
void foo() {}
EOF

rm -f $t/libfoo.a
ar rcu $t/libfoo.a $t/a.o

cat <<EOF | $CC -c -o $t/b.o -fPIC -xc -
void bar() {}
EOF

rm -f $t/libbar.a
ar rcu $t/libbar.a $t/b.o

cat <<EOF | $CC -c -o $t/c.o -xc -
void foo();
void bar();

void baz() {
  foo();
  bar();
}
EOF

clang --ld-path=./ld64 -shared -o $t/f.dylib $t/c.o -L$t -lfoo -Wl,-hidden-lbar

nm -g $t/f.dylib > $t/log
grep -q ' _foo$' $t/log
! grep -q ' _bar$' $t/log || false
grep -q ' _baz$' $t/log

echo OK
