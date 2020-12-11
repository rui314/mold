#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d %d\n", foo, bar, &foo == &bar);
  return 0;
}
EOF

cat <<EOF | cc -o $t/b.o -c -x assembler -
  .globl foo, bar
  .data;
foo:
bar:
  .long 42
EOF

../mold -o $t/exe /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
  $t/a.o $t/b.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
  /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 \
  /lib/x86_64-linux-gnu/libc.so.6 \
  /usr/lib/x86_64-linux-gnu/libc_nonshared.a \
  /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2 \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o > /dev/null

$t/exe | grep -q '42 42 1'

echo ' OK'
