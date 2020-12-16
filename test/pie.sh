#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

../mold -pie -o $t/exe \
  -dynamic-linker /lib64/ld-linux-x86-64.so.2 \
  /usr/lib/x86_64-linux-gnu/Scrt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/10/crtbeginS.o \
  -L/usr/lib/gcc/x86_64-linux-gnu/10 \
  -L/usr/lib/x86_64-linux-gnu \
  -L/usr/lib64 \
  -L/lib/x86_64-linux-gnu \
  -L/lib64 \
  -L/usr/lib/x86_64-linux-gnu \
  -L/usr/lib64 \
  -L/usr/lib64 \
  -L/usr/lib \
  -L/usr/lib/llvm-10/lib \
  -L/lib \
  -L/usr/lib \
  /home/ruiu/mold/test/tmp/pie/a.o \
  -lgcc \
  -lgcc_s \
  -lc \
  -lgcc \
  -lgcc_s \
  /usr/lib/gcc/x86_64-linux-gnu/10/crtendS.o \
  /usr/lib/x86_64-linux-gnu/crtn.o

$t/exe | grep -q 'Hello world'

echo ' OK'
