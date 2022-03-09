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

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .text
  .globl _start
_start:
  nop
EOF

"$mold" -o $t/b.so $t/a.o -auxiliary foo -f bar -shared

readelf --dynamic $t/b.so > $t/log
fgrep -q 'Auxiliary library: [foo]' $t/log
fgrep -q 'Auxiliary library: [bar]' $t/log

echo OK
