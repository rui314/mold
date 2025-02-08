#!/bin/bash
. $(dirname $0)/common.inc

# Skip if libc is musl
is_musl && skip

cat <<'EOF' | $CC -c -o $t/a.o -x assembler -
.globl fn1
fn1:
  sub $8, %rsp
  movabs ptr, %rax
  call *%rax
  add $8, %rsp
  ret
EOF

cat <<EOF | $CC -c -o $t/b.o -fPIC -xc -
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

$CC -B. -pie -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep 3

readelf --dynamic $t/exe | grep -F '(TEXTREL)'
readelf --dynamic $t/exe | grep '\(FLAGS\).*TEXTREL'
