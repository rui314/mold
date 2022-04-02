#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .text
  .globl foo
  .hidden foo
foo:
  nop
  .globl bar
bar:
  nop
  .globl _start
_start:
  nop
EOF

$CC -shared -fPIC -o $t/b.so -xc /dev/null
"$mold" -o $t/exe $t/a.o $t/b.so --export-dynamic

readelf --dyn-syms $t/exe > $t/log
grep -Pq 'NOTYPE  GLOBAL DEFAULT    \d+ bar' $t/log
grep -Pq 'NOTYPE  GLOBAL DEFAULT    \d+ _start' $t/log

echo OK
