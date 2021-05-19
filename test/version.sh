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

echo OK
