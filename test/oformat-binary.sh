#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIE
void _start() {}
EOF

./mold -o $t/exe $t/a.o --oformat=binary -Ttext=0x4000 -Map=$t/map
grep -E '^\s+0x4000\s+[0-9]+\s+[0-9]+\s+\.text$' $t/map

not grep -F .strtab $t/map
not grep -F .shstrtab $t/map
not grep -F .symtab $t/map
not grep -F .comment $t/map
