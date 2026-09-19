#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
__attribute__((visibility("hidden"))) int hidden_fn() { return 7; }
int visible_fn() { return 8; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int hidden_fn();
int visible_fn();
int main() { printf("%d %d\n", hidden_fn(), visible_fn()); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '7 8'

# The hidden symbol must not be exported
dyld_info -exports $t/exe > $t/exports
grep -q _visible_fn $t/exports
not grep -q _hidden_fn $t/exports

# ... but should still be in the symbol table as a local
nm $t/exe | grep -q 't _hidden_fn'
