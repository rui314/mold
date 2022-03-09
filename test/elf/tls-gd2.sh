#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

if [ "$(uname -m)" = x86_64 ]; then
  dialect=gnu
elif [ "$(uname -m)" = aarch64 ]; then
  dialect=trad
else
  echo skipped
  exit 0
fi

echo '{ global: bar; local: *; };' > $t/a.ver

cat <<EOF | gcc -mtls-dialect=$dialect -fPIC -c -o $t/b.o -xc -
_Thread_local int foo;

int bar() {
  return foo;
}
EOF

$CC -B. -shared -o $t/c.so $t/b.o -Wl,--version-script=$t/a.ver \
  -Wl,--no-relax

readelf -W --dyn-syms $t/c.so | grep -Eq 'TLS     LOCAL  DEFAULT .* foo'

echo OK
