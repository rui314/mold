#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

[ "`uname -p`" = arm ] && { echo skipped; exit; }

cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-headerpad,0
cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-headerpad,0x10000

echo OK
