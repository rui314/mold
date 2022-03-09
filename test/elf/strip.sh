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

cat <<'EOF' | $CC -x assembler -c -o $t/a.o -Wa,-L -
.globl _start, foo
_start:
foo:
bar:
.L.baz:
EOF

"$mold" -o $t/exe $t/a.o
readelf --symbols $t/exe > $t/log
fgrep -q _start $t/log
fgrep -q foo $t/log
fgrep -q bar $t/log
fgrep -q .L.baz $t/log

"$mold" -o $t/exe $t/a.o -strip-all
readelf --symbols $t/exe > $t/log
! fgrep -q _start $t/log || false
! fgrep -q foo $t/log || false
! fgrep -q bar $t/log || false
! fgrep -q .L.baz $t/log || false

echo OK
