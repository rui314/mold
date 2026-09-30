#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-macos_version_min,10.9
otool -l $t/exe1 > $t/log
grep -q 'platform 1' $t/log
grep -q 'minos 10.9' $t/log
# The legacy flag carries no separate SDK; ld64 records it as the version.
grep -q 'sdk 10.9' $t/log
