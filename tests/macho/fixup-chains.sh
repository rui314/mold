#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int x = 5;
int *p = &x;
int (*pf)(const char *, ...) = printf;
int main() {
  pf("%d\n", *p);
}
EOF2

# Modern deployment targets default to chained fixups
$CC --ld-path=$mold -o $t/exe $t/a.o
otool -l $t/exe | grep -q LC_DYLD_CHAINED_FIXUPS
$t/exe | grep '^5$'
dyld_info -fixups $t/exe > $t/fixups
grep -q 'bind.*_printf' $t/fixups
grep -q 'rebase' $t/fixups

# -no_fixup_chains falls back to classic dyld info
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_fixup_chains
otool -l $t/exe2 > $t/lc
not grep -q LC_DYLD_CHAINED_FIXUPS $t/lc
grep -q LC_DYLD_INFO_ONLY $t/lc
$t/exe2 | grep '^5$'
