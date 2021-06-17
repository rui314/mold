#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | clang -c -o $t/a.o -x assembler -
.globl fn1
fn1:
  movabs ptr, %rax
  call *%rax
  ret
EOF

cat <<EOF | clang -c -o $t/b.o -xc -
#include <stdio.h>

int fn1();

int fn2() {
  return 3;
}

void *ptr = fn2;

int main() {
  printf("%d\n", fn1());
}
EOF

clang -fuse-ld=`pwd`/../mold -pie -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 3

readelf --dynamic $t/exe | fgrep -q '(TEXTREL)'
readelf --dynamic $t/exe | grep -q '\(FLAGS\).*TEXTREL'

echo OK
