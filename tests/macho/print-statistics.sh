#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-print_statistics 2> $t/log
$t/exe
grep -q 'ld total time:' $t/log
grep -q 'copy+write' $t/log
grep -Eq 'objects: 1 alive of [0-9]+' $t/log

# Off by default.
$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log2
! grep -q 'ld total time' $t/log2 || false
