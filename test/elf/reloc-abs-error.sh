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

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl foo
foo = 0x800000
EOF

cat <<EOF | $CC -o $t/b.o -c -fPIC -xc -
void foo();
int main() { foo(); }
EOF

! $CC -B. -o $t/exe -pie $t/a.o $t/b.o >& $t/log
grep -q 'recompile with -fno-PIC' $t/log

echo OK
