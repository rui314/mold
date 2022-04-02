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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

"$mold" -e foo -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201000" $t/log

"$mold" -e bar -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201008" $t/log

"$mold" -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q "Entry point address:.*0x201000" $t/log

echo OK
