#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl
otool -l $t/exe1 | grep DATA_IN_CODE

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-data_in_code_info
otool -l $t/exe2 | grep DATA_IN_CODE

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-no_data_in_code_info
otool -l $t/exe3 > $t/log3
! grep -q DATA_IN_CODE $t/log3 || false
# Entries from input objects are carried over, rebased to output file
# offsets.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _start2
_start2:
  ret
.data_region jt32
  .long 1
  .long 2
.end_data_region
EOF

$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/b.o
otool -G $t/exe4 > $t/log4
grep -q '(1 entries)' $t/log4
grep -Eq '^0x[0-9a-f]+ +8 +0x0004' $t/log4

# A -r output carries them at their merged addresses, which is what an
# object's entries hold (ld-prime): the jump table sits one instruction
# past _start2.
$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
otool -G $t/r.o > $t/log5
grep -q '(1 entries)' $t/log5
start2=0x$(nm $t/r.o | awk '$3 == "_start2" {print $1}')
insn=$([ $ARCH = arm64 ] && echo 4 || echo 1)
grep -Eq "^$(printf '0x%08x' $((start2 + insn))) +8 +0x0004" $t/log5
