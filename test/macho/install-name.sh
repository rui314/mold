#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
void foo() {}
EOF

cc --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o -Wl,-install_name,foobar
otool -l $t/b.dylib | grep -q 'name foobar'

echo OK
