#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  call foo
EOF

! $mold -o $t/exe $t/a.o 2> $t/log || false
grep -q 'undefined symbol: .*\.o: foo' $t/log

echo OK
