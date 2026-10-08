#!/usr/bin/env bash
. $(dirname $0)/common.inc

# -r preserves SHF_LINK_ORDER sections with their links.
cat <<EOF | $CC -c -o $t/a.o -xc - -ffunction-sections -fpatchable-function-entry=1
int foo() { return 3; }
int bar() { return 5; }
int main() { return foo() - 3; }
EOF

# Old toolchains don't set SHF_LINK_ORDER on __patchable_function_entries.
readelf -SW $t/a.o | grep -E '__patchable_function_entries .* WAL ' || skip

./mold -r -o $t/b.o $t/a.o
readelf -SW $t/b.o | grep -F __patchable_function_entries > $t/log
grep -E ' WAL +[1-9]' $t/log
not grep -E ' WAL +0 ' $t/log
