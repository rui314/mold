#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

echo 'VERSION { ver_x /*' > $t/b.script

not $CC -B. -o $t/exe $t/a.o $t/b.script |& grep -q 'unclosed comment'
