#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

../mold -v | grep -Pq 'mold .*\; compatible with GNU ld\)'
../mold --version | grep -Pq 'mold .*\; compatible with GNU ld\)'

../mold -V | grep -Pq 'mold .*\; compatible with GNU ld\)'
../mold -V | grep -q elf_x86_64
../mold -V | grep -q elf_i386

cat <<EOF | clang -c -xc -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld=`pwd`/../mold -Wl,--version -o $t/exe $t/a.o | grep -q mold
$t/exe | grep -q 'Hello world'

echo OK
