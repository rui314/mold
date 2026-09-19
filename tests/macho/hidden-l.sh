#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int three() { return 3; }
EOF2

rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int three();
int main() { printf("%d\n", three()); }
EOF2

$CC --ld-path=$mold -shared -o $t/lib.dylib $t/b.o -L$t -Wl,-hidden-lfoo \
  -Wl,-undefined,dynamic_lookup -e _main 2>/dev/null || true
$CC --ld-path=$mold -o $t/exe $t/b.o -L$t -Wl,-hidden-lfoo
$t/exe | grep '^3$'

# The archive's symbol resolves but is not exported and shows as a
# local in the symbol table.
dyld_info -exports $t/exe > $t/exports
not grep -q _three $t/exports
nm $t/exe | grep -q 't _three'
