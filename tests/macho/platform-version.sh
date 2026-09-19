#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-platform_version,macos,13.5,12.0

otool -l $t/exe > $t/log
grep -Fq 'minos 13.5' $t/log
grep -Fq 'sdk 12.0' $t/log
