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

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo();
int main() { foo(); }
EOF

cmd=(cc -B. -o $t/exe $t/a.o)

! "${cmd[@]}" 2>&1 | grep -q 'undefined.*foo'
! "${cmd[@]}" -Wl,-unresolved-symbols=report-all 2>&1 | grep -q 'undefined.*foo'

"${cmd[@]}" -Wl,-unresolved-symbols=ignore-all

! readelf --dyn-syms $t/exe | grep -w foo || false

"${cmd[@]}" -Wl,-unresolved-symbols=report-all -Wl,--warn-unresolved-symbols 2>&1 | \
  grep -q 'undefined.*foo'

! "${cmd[@]}" -Wl,-unresolved-symbols=ignore-in-object-files 2>&1 | grep -q 'undefined.*foo'
! "${cmd[@]}" -Wl,-unresolved-symbols=ignore-in-shared-libs 2>&1 | grep -q 'undefined.*foo'

echo OK
