#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF > $t/a.c
int compute(int x) { return x * 7; }
EOF

cat <<EOF > $t/b.c
#include <stdio.h>
int compute(int);
int main() { printf("%d\n", compute(6)); }
EOF

$CC -g -c $t/a.c -o $t/a.o
$CC -g -c $t/b.c -o $t/b.o

# Like ld64, a -r link does not merge DWARF (its section-relative
# offsets carry no relocations); the merged object gets debug-note
# stabs naming the input objects, and a final link carries those notes
# through, so the executable's N_OSO entries name a.o and b.o.
$mold -r -arch $ARCH -platform_version macos 15.0 15.0 -o $t/merged.o $t/a.o $t/b.o
otool -l $t/merged.o > $t/lc
not grep -q '__debug_info' $t/lc
nm -pa $t/merged.o > $t/stabs
grep -q 'OSO.*/a.o' $t/stabs
grep -q 'OSO.*/b.o' $t/stabs
grep -q 'FUN _compute' $t/stabs

$CC --ld-path=$mold -g -o $t/exe $t/merged.o
$t/exe | grep -q '^42$'
nm -pa $t/exe > $t/stabs2
grep -q 'OSO.*/a.o' $t/stabs2
grep -q 'OSO.*/b.o' $t/stabs2
not grep -q 'OSO.*merged.o' $t/stabs2

# Apple's linker accepts the merged object too.
$CC -g -o $t/exe2 $t/merged.o
$t/exe2 | grep -q '^42$'

# lldb sets a source-level breakpoint in code that came through -r.
lldb -b -o 'b compute' -o run -o 'p x' $t/exe > $t/lldb.log 2>&1 || true
grep -q 'stop reason = breakpoint' $t/lldb.log
grep -q '(int) 6' $t/lldb.log
