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

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl _start
_start:
EOF

"$mold" -o $t/exe $t/a.o

readelf --sections $t/exe > $t/log
! fgrep .interp $t/log || false

readelf --dynamic $t/exe > $t/log

"$mold" -o $t/exe $t/a.o --dynamic-linker=/foo/bar

readelf --sections $t/exe > $t/log
fgrep -q .interp $t/log

echo OK
