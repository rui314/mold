#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

# Skip if libc is musl
echo 'int main() {}' | cc -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

# Skip if target is not x86-64
[ $(uname -m) = x86_64 ] || { echo skipped; exit; }

cat <<'EOF' | clang -c -o $t/a.o -x assembler -
.globl fn1
fn1:
  sub $8, %rsp
  movabs ptr, %rax
  call *%rax
  add $8, %rsp
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

clang -fuse-ld=$mold -pie -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 3

readelf --dynamic $t/exe | fgrep -q '(TEXTREL)'
readelf --dynamic $t/exe | grep -q '\(FLAGS\).*TEXTREL'

echo OK
