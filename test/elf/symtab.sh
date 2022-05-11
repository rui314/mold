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

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .globl foo, bar, this_is_global
local1:
foo:
bar:
  .byte 0
EOF

cat <<EOF | $CC -o $t/b.o -c -x assembler -
  .globl this_is_global
local2:
this_is_global:

  .globl module_local
module_local:
EOF

echo '{ local: module_local; };' > $t/c.map

./mold -o $t/exe $t/a.o $t/b.o --version-script=$t/c.map

readelf --symbols $t/exe > $t/log

grep -Eq '0 NOTYPE  LOCAL  DEFAULT .* local1' $t/log
grep -Eq '0 NOTYPE  LOCAL  DEFAULT .* local2' $t/log
grep -Eq '0 NOTYPE  GLOBAL DEFAULT .* foo' $t/log
grep -Eq '0 NOTYPE  GLOBAL DEFAULT .* bar' $t/log
grep -Eq '0 NOTYPE  GLOBAL DEFAULT .* this_is_global' $t/log
grep -Eq '0 NOTYPE  GLOBAL DEFAULT .* module_local' $t/log

echo OK
