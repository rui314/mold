#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo <<EOF | clang -fuse-ld=`pwd`/../mold -o $t/a.so -shared -x assembler -
.globl expfn
expfn:
  nop
EOF

echo OK
