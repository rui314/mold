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

cat <<EOF | gcc -mtls-dialect=$dialect -c -o $t/a.o -xc -
#include <stdio.h>

extern _Thread_local int foo;
static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  foo = 3;
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | $CC -xc -c -o $t/b.o -
_Thread_local int foo;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '3 5 3 5'

echo OK
