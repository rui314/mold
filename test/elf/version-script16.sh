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

cat <<'EOF' > $t/a.ver
{ local: *; global: extern "C++" { *foo*; }; };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
void foobar() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf --dyn-syms $t/c.so | grep -q foobar

echo OK
