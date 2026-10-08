#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-Map=$t/map
grep -E '^ +0x[0-9a-f]+ +[0-9]+ +[0-9]+ \.text$' $t/map

# Non-allocated sections are at address 0, which is written as "0".
grep -E '^ +0 +[0-9]+ +[0-9]+ \.symtab$' $t/map
