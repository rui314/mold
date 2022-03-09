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

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.section .data.foo.1,"aw",@progbits
.ascii "a"
.section .data.foo.1,"aw",@progbits
.ascii "b"
.section .data.foo.2,"aw",@progbits
.ascii "c"
.section .data.bar.1,"aw",@progbits
.ascii "d"
.section .data.bar.2,"aw",@progbits
.ascii "e"
.text
.globl _start
_start:
  nop
EOF

$CC -B. -o $t/exe $t/a.o -nostdlib -Wl,-unique='*foo*'

readelf -x .data.foo.1 $t/exe | grep -q ab
readelf -x .data.foo.2 $t/exe | grep -q c
readelf -x .data $t/exe | grep -q de

echo OK
