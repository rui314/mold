#!/bin/bash
set -e
mold=$1
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

$mold -o $t/b.so $t/a.o -auxiliary foo -f bar -shared

readelf --dynamic $t/b.so > $t/log
fgrep -q 'Auxiliary library: [foo]' $t/log
fgrep -q 'Auxiliary library: [bar]' $t/log

echo OK
