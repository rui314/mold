#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

echo 'INCLUDE inner.script' > $t/outer.script
echo 'foo = 0xbeef;' > $t/inner.script

$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/outer.script
readelf -sW $t/exe | grep -E 'beef .* ABS foo'

# An error in an included file should be reported with the name of
# the included file.
echo 'INCLUDE bad.script' > $t/outer2.script
echo 'NO_SUCH_COMMAND' > $t/bad.script

not $CC -B. -o $t/exe2 $t/a.o -Wl,-T,$t/outer2.script |& grep 'bad.script:1:'
