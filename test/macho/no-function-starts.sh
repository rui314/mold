#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -q LC_FUNCTION_STARTS

cc --ld-path=./ld64 -o $t/exe2 $t/a.o -Wl,-no_function_starts
otool -l $t/exe2 > $t/log
! grep -q LC_FUNCTION_STARTS $t/log || false

echo OK
