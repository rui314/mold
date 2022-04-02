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

echo 'int main() { return 0; }' > $t/a.c
echo 'int main() { return 0; }' > $t/b.c

! $CC -B. -o $t/exe $t/a.c $t/b.c 2> /dev/null || false
$CC -B. -o $t/exe $t/a.c $t/b.c -Wl,-allow-multiple-definition
$CC -B. -o $t/exe $t/a.c $t/b.c -Wl,-z,muldefs

echo OK
