#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-add_empty_section,__FOO,__foo

otool -l $t/exe | grep 'segname __FOO'
otool -l $t/exe | grep 'sectname __foo'
# The segment of only such anchors is flagged SG_NORELOC, as ld-prime
# flags it.
otool -l $t/exe | grep -A9 'segname __FOO' | grep 'flags 0x4'
