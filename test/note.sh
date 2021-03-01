#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl _start
_start:
  nop

  .section .note.foo, "", @note
  .align 8
  .quad 42

  .section .note.bar, "", @note
  .align 4
  .quad 42

  .section .note.baz, "", @note
  .align 8
  .quad 42
EOF

../mold -static -o $t/exe $t/a.o

readelf --section-header $t/exe | fgrep -q '
Section Headers:
  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al
  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0
  [ 1] .note.bar         NOTE            0000000000000000 0001c8 000008 00      0   0  4
  [ 2] .note.baz         NOTE            0000000000000000 0001d0 000008 00      0   0  8
'

readelf --segments $t/exe | fgrep -q '
 Section to Segment mapping:
  Segment Sections...
   00
   01     .note.bar
   02     .note.baz .note.foo
'

echo OK
