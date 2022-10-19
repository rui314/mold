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

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-flat_namespace
otool -hv $t/exe | grep -qv TWOLEVEL

cc --ld-path=./ld64 -o $t/exe $t/a.o
otool -hv $t/exe | grep -v TWOLEVEL

echo OK
