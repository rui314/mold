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

readelf --dyn-syms $t/b.so | grep -q "
Symbol table '.dynsym' contains 5 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     0: 0000000000000000     0 NOTYPE  LOCAL  DEFAULT  UND
     1: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND __cxa_finalize@GLIBC_2.2.5 (2)
     2: 0000000000000000     0 NOTYPE  WEAK   DEFAULT  UND __gmon_start__
     3: 0000000000000000     0 NOTYPE  WEAK   DEFAULT  UND _ITM_deregisterTMCloneTable
     4: 0000000000000000     0 NOTYPE  WEAK   DEFAULT  UND _ITM_registerTMCloneTable
     5: 000000000000110c     0 NOTYPE  GLOBAL DEFAULT   15 fn1
"

echo OK
