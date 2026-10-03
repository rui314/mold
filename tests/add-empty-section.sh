#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-add_empty_section,__FOO,__foo

otool -l $t/exe | grep 'segname __FOO'
otool -l $t/exe | grep 'sectname __foo'
$t/exe

# A segment of only -sectcreate contents, and one that also holds an
# input section.
echo hello > $t/blob
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-sectcreate,__FOO,__blob,$t/blob
otool -l $t/exe2 | grep -A1 'sectname __blob' | grep -q 'segname __FOO'
otool -s __FOO __blob $t/exe2 | grep -Eq '6c6c6568 6f 0a|68 65 6c 6c 6f 0a'
$t/exe2
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __FOO,__obj
.quad 7
EOF
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o -Wl,-sectcreate,__FOO,__blob,$t/blob
otool -l $t/exe3 | grep -A1 'sectname __blob' | grep -q 'segname __FOO'
otool -l $t/exe3 | grep -A1 'sectname __obj' | grep -q 'segname __FOO'
$t/exe3

# The sections of -add_empty_section and -sectcreate come in
# command-line order, the two options' interleaved, and so do the
# segments only they make.
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-add_empty_section,__BAR,__e1 \
  -Wl,-sectcreate,__FOO,__s1,$t/blob -Wl,-add_empty_section,__FOO,__e2 \
  -Wl,-sectcreate,__FOO,__s2,$t/blob -Wl,-add_empty_section,__BAR,__e3
otool -l $t/exe4 | grep -A1 'sectname __[es][0-9]' | grep -v -- -- | paste - - |
  awk '{print $4 "," $2}' | tr '\n' ' ' > $t/log4
grep -q '^__BAR,__e1 __BAR,__e3 __FOO,__s1 __FOO,__e2 __FOO,__s2 $' $t/log4
