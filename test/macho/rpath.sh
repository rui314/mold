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

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-rpath,foo -Wl,-rpath,@bar
otool -l $t/exe > $t/log

grep -A3 'cmd LC_RPATH' $t/log | grep -q 'path foo'
grep -A3 'cmd LC_RPATH' $t/log | grep -q 'path @bar'

echo OK
