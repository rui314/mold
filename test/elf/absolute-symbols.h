#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo, main
foo = 0x8080
main:
  call foo
EOF

$CC -B. -o $t/exe $t/a.o
objdump -d $t/exe | grep -qE 'callq\s+8080'

echo OK
