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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

if [ $MACHINE = x86_64 ]; then
  mtls=-mtls-dialect=gnu
elif [ $MACHINE = aarch64 ]; then
  mtls=-mtls-dialect=trad
elif [ $MACHINE '!=' riscv64 -a $MACHINE '!=' riscv32 ]; then
  echo skipped
  exit
fi

echo '{ global: bar; local: *; };' > $t/a.ver

cat <<EOF | $GCC $mtls -fPIC -c -o $t/b.o -xc -
_Thread_local int foo;

int bar() {
  return foo;
}
EOF

$CC -B. -shared -o $t/c.so $t/b.o -Wl,--version-script=$t/a.ver \
  -Wl,--no-relax

readelf -W --dyn-syms $t/c.so | grep -Eq 'TLS     LOCAL  DEFAULT .* foo'

echo OK
