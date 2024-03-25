#!/bin/bash
. $(dirname $0)/common.inc

echo endbr64 | $CC -o /dev/null -c -xassembler - 2> /dev/null || skip

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.globl main
main:
  call _exit@PLT
EOF

$CC -B. -o $t/exe $t/a.o
readelf --notes $t/exe > $t/log
! grep -qw SHSTK $t/log

$CC -B. -o $t/exe $t/a.o -Wl,-z,ibt
readelf --notes $t/exe | grep -qw IBT
