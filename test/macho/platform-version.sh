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

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-platform_version,macos,13.5,12.0

otool -l $t/exe > $t/log
grep -Fq 'minos 13.5' $t/log
grep -Fq 'sdk 12.0' $t/log

echo OK
