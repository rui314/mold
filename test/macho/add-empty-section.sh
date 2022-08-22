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

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-add_empty_section,__FOO,__foo

otool -l $t/exe | grep -q 'segname __FOO'
otool -l $t/exe | grep -q 'sectname __foo'

echo OK
