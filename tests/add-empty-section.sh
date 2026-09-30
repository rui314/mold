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

# So is a segment of only -sectcreate contents, but not one that also
# holds an input section.
echo hello > $t/blob
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-sectcreate,__FOO,__blob,$t/blob
otool -l $t/exe2 | grep -A9 'segname __FOO' | grep 'flags 0x4'
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __FOO,__obj
.quad 7
EOF
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-sectcreate,__FOO,__blob,$t/blob
otool -l $t/exe3 | grep -A9 'segname __FOO' | grep 'flags 0x0'
