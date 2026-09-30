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

# The warnings from checking the options obey -w too, wherever it
# appears: -no_pie draws one on arm64, and on x86-64 when it targets
# macOS 13 or later.
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/b.o -mmacosx-version-min=14.0 -Wl,-no_pie >& $t/log3
grep -q warning $t/log3
$CC --ld-path=$mold -o $t/exe $t/b.o -mmacosx-version-min=14.0 -Wl,-no_pie,-w >& $t/log4
not grep -q warning $t/log4
