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
EOF

../mold -o $t/exe $t/a.o -auxiliary foo -f bar

readelf --dynamic $t/exe > $t/log
fgrep -q 'Auxiliary library: [foo]' $t/log
fgrep -q 'Auxiliary library: [bar]' $t/log

echo OK
