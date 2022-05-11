#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<'EOF' | $CC -o $t/a.o -c -x assembler -
.globl _start
.text
_start:
  ret

.section .text.hot
.ascii ".text.hot "
.section .text.hot.foo
.ascii ".text.hot.foo "

.section .text.unknown
.ascii ".text.unknown "
.section .text.unknown.foo
.ascii ".text.unknown.foo "

.section .text.unlikely
.ascii ".text.unlikely "
.section .text.unlikely.foo
.ascii ".text.unlikely.foo "

.section .text.startup
.ascii ".text.startup "
.section .text.startup.foo
.ascii ".text.startup.foo "

.section .text.exit
.ascii ".text.exit "
.section .text.exit.foo
.ascii ".text.exit.foo "

.section .text
.ascii ".text "
.section .text.foo
.ascii ".text.foo "

.section .data.rel.ro
.ascii ".data.rel.ro "
.section .data.rel.ro.foo
.ascii ".data.rel.ro.foo "

.section .data
.ascii ".data "
.section .data.foo
.ascii ".data.foo "

.section .rodata
.ascii ".rodata "
.section .rodata.foo
.ascii ".rodata.foo "
EOF

./mold -o $t/exe $t/a.o -z keep-text-section-prefix

readelf -p .text.hot $t/exe | fgrep -q '.text.hot .text.hot.foo'
readelf -p .text.unknown $t/exe | fgrep -q '.text.unknown .text.unknown.foo'
readelf -p .text.unlikely $t/exe | fgrep -q '.text.unlikely .text.unlikely.foo'
readelf -p .text.startup $t/exe | fgrep -q '.text.startup .text.startup.foo'
readelf -p .text.exit $t/exe | fgrep -q '.text.exit .text.exit.foo'
readelf -p .text $t/exe | fgrep -q '.text .text.foo'
readelf -p .data.rel.ro $t/exe | fgrep -q '.data.rel.ro .data.rel.ro.foo'
readelf -p .data $t/exe | fgrep -q '.data .data.foo'
readelf -p .rodata $t/exe | fgrep -q '.rodata .rodata.foo'

./mold -o $t/exe $t/a.o
! readelf --sections $t/exe | fgrep -q .text.hot || false

./mold -o $t/exe $t/a.o -z nokeep-text-section-prefix
! readelf --sections $t/exe | fgrep -q .text.hot || false

echo OK
