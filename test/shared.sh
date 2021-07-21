#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | clang -fPIC -c -o $t/a.o -x assembler -
.globl fn1, fn2, fn3
fn1:
  sub $8, %rsp
  call fn2@PLT
  add $8, %rsp
  ret
fn3:
  nop
EOF

clang -shared -fuse-ld=$mold -o $t/b.so $t/a.o

readelf --dyn-syms $t/b.so > $t/log

grep -q '0000000000000000     0 NOTYPE  GLOBAL DEFAULT  UND fn2' $t/log
grep -Pq '0 NOTYPE  GLOBAL DEFAULT   \d+ fn1' $t/log

cat <<EOF | clang -fPIC -c -o $t/c.o -xc -
#include <stdio.h>

int fn1();

void fn2() {
  printf("hello\n");
}

int main() {
  fn1();
  return 0;
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.o $t/b.so
$t/exe | grep -q hello
! readelf --symbols $t/exe | grep -q fn3 || false

echo OK
