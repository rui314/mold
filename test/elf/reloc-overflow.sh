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

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .globl foo
  .data
foo:
  .short foo
EOF

! "$mold" -e foo -static -o $t/exe $t/a.o 2> $t/log || false
fgrep -q 'relocation R_X86_64_16 against foo out of range' $t/log

echo OK
