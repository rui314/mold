#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

echo 'foobar' > $t/contents

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectcreate,__TEXT,__foo,$t/contents
$RUN $t/exe
otool -l $t/exe | grep -A3 'sectname __foo' > $t/log
grep -q 'segname __TEXT' $t/log
grep -q 'size 0x0*7$' $t/log

# Longer names than a section header holds are cut to 16 bytes, with a
# warning.
$CC --ld-path=$mold -o $t/exe2 $t/a.o \
  -Wl,-sectcreate,__SCSCSCSCSCSCSCSCSC,__scscscscscscscscscsc,$t/contents 2> $t/log2
grep -q "warning: -sectcreate segment name too long ('__SCSCSCSCSCSCSCSCSC'), will be truncated to '__SCSCSCSCSCSCSC'" $t/log2
grep -q "warning: -sectcreate section name too long ('__scscscscscscscscscsc'), will be truncated to '__scscscscscscsc'" $t/log2
otool -l $t/exe2 | grep -A1 'sectname __scscscscscscsc$' | grep 'segname __SCSCSCSCSCSCSC$'
$RUN $t/exe2
