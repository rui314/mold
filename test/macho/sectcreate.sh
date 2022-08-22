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

echo 'foobar' > $t/contents

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-sectcreate,__TEXT,__foo,$t/contents

otool -l $t/exe | grep -A3 'sectname __foo' > $t/log
grep -q 'segname __TEXT' $t/log
grep -q 'segname __TEXT' $t/log
grep -q 'size 0x0*7$' $t/log

echo OK
