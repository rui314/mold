#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

rm -f $t/a.o
touch $t/a.o
! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o &> $t/log || false
grep -q 'unknown file type: EMPTY' $t/log

echo OK
