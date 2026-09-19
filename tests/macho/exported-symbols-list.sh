#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
int bar() { return 2; }
int main() { return 0; }
EOF2

cat <<EOF2 > $t/list
# only foo is exported
_foo
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-exported_symbols_list,$t/list
dyld_info -exports $t/exe > $t/exports
grep -q _foo $t/exports
not grep -q _bar $t/exports
