#!/bin/bash
source "$(dirname "$0")"/common.inc

# -w suppresses warnings. An object built for a newer macOS than the
# link targets draws one from both mold and ld-prime.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=15.0
void foo() {}
EOF

$CC --ld-path=$mold -shared -o $t/d.so $t/a.o -mmacosx-version-min=14.0 >& $t/log1

grep -q warning $t/log1

$CC --ld-path=$mold -shared -o $t/d.so $t/a.o -mmacosx-version-min=14.0 -Wl,-w >& $t/log2

not grep -q warning $t/log2
