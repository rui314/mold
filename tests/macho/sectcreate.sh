#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

echo 'foobar' > $t/contents

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectcreate,__TEXT,__foo,$t/contents
$t/exe
otool -l $t/exe | grep -A3 'sectname __foo' > $t/log
grep -q 'segname __TEXT' $t/log
grep -q 'size 0x0*7$' $t/log
