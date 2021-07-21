#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
.globl foo, bar
foo:
  .quad 0
bar:
  .quad 0
EOF

$mold -e foo -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q 'Entry point address:.*0x201000' $t/log

$mold -e bar -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q 'Entry point address:.*0x201008' $t/log

$mold -static -o $t/exe $t/a.o
readelf -e $t/exe > $t/log
grep -q 'Entry point address:.*0x201000' $t/log

echo OK
