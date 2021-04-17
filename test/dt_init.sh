#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -x assembler -
.globl main, init, fini
main:
  ret
init:
  ret
fini:
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf -a $t/exe > $t/log

grep -Pq '\(INIT\)\s+0x233010' $t/log
grep -Pq '\(FINI\)\s+0x233000' $t/log
grep -Pq '0000000000233010\s+0 FUNC    GLOBAL HIDDEN    \d+ _init$' $t/log
grep -Pq '0000000000233000\s+0 FUNC    GLOBAL HIDDEN    \d+ _fini$' $t/log

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-init,init -Wl,-fini,fini
readelf -a $t/exe > $t/log

grep -Pq '\(INIT\)\s+0x233117' $t/log
grep -Pq '\(FINI\)\s+0x233118' $t/log
grep -Pq '0000000000233117\s+0 NOTYPE  GLOBAL DEFAULT   \d+ init$' $t/log
grep -Pq '0000000000233118\s+0 NOTYPE  GLOBAL DEFAULT   \d+ fini$' $t/log

echo OK
