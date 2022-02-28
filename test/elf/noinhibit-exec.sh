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

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo();
int main() { foo(); }
EOF

! $CC -B. -o $t/exe $t/a.o >& $t/log1 || false
grep -q 'error: undefined symbol' $t/log1

$CC -B. -o $t/exe $t/a.o -Wl,-noinhibit-exec >& $t/log2
grep -q 'warning: undefined symbol' $t/log2

echo OK
