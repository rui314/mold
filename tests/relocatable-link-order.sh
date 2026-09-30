#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc - -ffunction-sections -fpatchable-function-entry=1
int foo() { return 3; }
int bar() { return 5; }
int main() { return foo() - 3; }
EOF

./mold -r -o $t/b.o $t/a.o
$CC -B. -o $t/exe $t/b.o -Wl,--gc-sections
$QEMU $t/exe
readelf -W --sections $t/exe | grep -F __patchable_function_entries
nm $t/exe | not grep -w bar
