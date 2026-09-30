#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
_Thread_local int counter = 5;
int bump() { return ++counter; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o \
  -install_name $PWD/$t/libfoo.dylib

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
extern _Thread_local int counter;
int bump();
int main() {
  printf("%d %d %d\n", counter, bump(), counter);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o $t/libfoo.dylib
$t/exe | grep '^5 6 6$'
# The imported descriptor's pointer is an ordinary GOT slot, as
# ld-prime lays it out: no __thread_ptrs section.
dyld_info -fixups $t/exe > $t/fixups
grep -q '__got .* bind .*libfoo/_counter' $t/fixups
otool -l $t/exe > $t/lc
not grep -q __thread_ptrs $t/lc
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/libfoo.dylib -Wl,-no_fixup_chains
$t/exe2 | grep '^5 6 6$'
dyld_info -fixups $t/exe2 > $t/fixups2
grep -q '__got .* bind .*libfoo/_counter' $t/fixups2
otool -l $t/exe2 > $t/lc2
not grep -q __thread_ptrs $t/lc2
