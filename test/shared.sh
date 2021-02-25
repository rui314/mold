#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -fPIC -c -o $t/a.o -x assembler -
.globl fn1, fn2
fn1:
  call fn2
EOF

clang -shared  -fuse-ld=`pwd`/../mold -o $t/b.so $t/a.o

readelf --dyn-syms $t/b.so > $t/log

grep -q '0000000000000000     0 NOTYPE  GLOBAL DEFAULT  UND fn2' $t/log
grep -q '000000000000111c     0 NOTYPE  GLOBAL DEFAULT   16 fn1' $t/log

echo OK
