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
VER_X1 { foo; };
VER_X2 { bar; };
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
int foo = 5;
int bar = 6;
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver \
  -o $t/c.so $t/b.o

cat <<'EOF' > $t/d.ver
VER_Y1 { local; *; };
VER_Y2 { baz; };
VER_Y3 { foo; };
EOF

cat <<EOF | $CXX -fPIC -c -o $t/e.o -xc -
extern int foo;
extern int bar;
int baz() { return foo + bar; }
EOF

$CC -B. -shared -Wl,-version-script,$t/d.ver \
  -o $t/f.so $t/e.o $t/c.so

readelf --dyn-syms $t/f.so > $t/log
grep -q 'foo@VER_X1' $t/log
grep -q 'bar@VER_X2' $t/log
grep -q 'baz@@VER_Y2' $t/log

echo OK
