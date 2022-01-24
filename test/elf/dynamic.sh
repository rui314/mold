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

echo '.globl main; main:' | $CC -o $t/a.o -c -x assembler -

$CC -B. -o $t/exe $t/a.o

readelf --dynamic $t/exe > $t/log
grep -Eq 'Shared library:.*\blibc.so\b' $t/log

readelf -W --dyn-syms --use-dynamic $t/exe > $t/log2
grep -Eq 'FUNC\s+GLOBAL\s+DEFAULT\s+UND\s+__libc_start_main' $t/log2

cat <<EOF | $CC -c -fPIC -o $t/b.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe -pie $t/b.o
count=$(readelf -W --relocs $t/exe | grep -E 'R_[a-zA-Z0-9_]+_RELATIVE' | wc -l)
readelf -W --dynamic $t/exe | grep -q "RELACOUNT.*\b$count\b"

echo OK
