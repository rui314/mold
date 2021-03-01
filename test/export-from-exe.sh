#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
.globl _start, expfn
_start:
expfn:
  nop
EOF

cat <<EOF | cc -shared -o $t/b.so -x assembler -
.globl x
x:
  call expfn@PLT
EOF

../mold -o $t/exe $t/a.o $t/b.so
readelf --dyn-syms $t/exe | grep -q expfn

echo OK
