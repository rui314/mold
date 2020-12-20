#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -Wa,--keep-locals -
  .text
  .globl _start
_start:
  nop
foo:
  nop
.Lbar:
  nop
EOF

../mold -o $t/exe $t/a.o
readelf --symbols $t/exe | fgrep -q foo
readelf --symbols $t/exe | fgrep -q .Lbar

../mold -o $t/exe $t/a.o --discard-locals
readelf --symbols $t/exe | fgrep -q foo
! readelf --symbols $t/exe | fgrep -q .Lbar

../mold -o $t/exe $t/a.o --discard-all
! readelf --symbols $t/exe | fgrep -q foo
! readelf --symbols $t/exe | fgrep -q .Lbar

echo ' OK'
