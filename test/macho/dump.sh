#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

echo 'int main() {}' | cc -o "$t"/exe -xc -
"$mold" -dump "$t"/exe > "$t"/log

grep -q 'magic: 0xfeedfacf' "$t"/log
grep -q 'segname: __PAGEZERO' "$t"/log

echo OK
