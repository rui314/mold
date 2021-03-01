#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | clang -c -o $t/a.o -x assembler -
.globl main
main:
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
readelf -a $t/exe > $t/log

grep -Pq '\(INIT\)\s+0x201020' $t/log
grep -Pq '\(FINI\)\s+0x201010' $t/log
grep -Pq '0000000000201020\s+0 FUNC    GLOBAL HIDDEN    \d+ _init$' $t/log
grep -Pq '0000000000201010\s+0 FUNC    GLOBAL HIDDEN    \d+ _fini$' $t/log

echo OK
