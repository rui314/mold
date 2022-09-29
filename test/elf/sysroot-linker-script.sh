#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

mkdir -p $t/foo/bar
rm -f $t/foo/bar/libfoo.a
ar rcs $t/foo/bar/libfoo.a $t/a.o

cat <<EOF > $t/foo/bar/b.script
INPUT(/foo/bar/libfoo.a)
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$t/ $t/foo/bar/b.script

echo OK
