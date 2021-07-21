#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
.globl _start, expfn1, expfn2
_start:
  nop
expfn1:
  nop
expfn2:
  nop
EOF

cat <<EOF | cc -shared -o $t/b.so -x assembler -
.globl x, expfn1, expfn2
x:
  call expfn1@PLT
expfn2:
  nop
EOF

$mold -o $t/exe $t/a.o $t/b.so
readelf --dyn-syms $t/exe | grep -q expfn2
readelf --dyn-syms $t/exe | grep -q expfn1

echo OK
