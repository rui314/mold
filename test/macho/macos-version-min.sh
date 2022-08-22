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

cc --ld-path=./ld64 -o $t/exe1 $t/a.o -Wl,-macos_version_min,10.9
otool -l $t/exe1 > $t/log
grep -q 'platform 1' $t/log
grep -q 'minos 10.9' $t/log

echo OK
