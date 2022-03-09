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
  dialect=gnu2
elif [ "$(uname -m)" = aarch64 ]; then
  dialect=desc
else
  echo skipped
  exit 0
fi

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;
extern _Thread_local int bar;

int main() {
  bar = 7;
  printf("%d %d\n", foo, bar);
}
EOF

cat <<EOF | gcc -fPIC -mtls-dialect=$dialect -shared -o $t/b.so -xc -
_Thread_local int foo = 5;
_Thread_local int bar;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
$t/exe | grep -q '5 7'

echo OK
