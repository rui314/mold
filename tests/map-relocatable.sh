#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r link writes a map as a final link does: of the output's
# sections, at the addresses they have from zero, and of their
# subsections. Those of the sections it makes itself are its objects':
# each record of __compact_unwind, named by the label at it if any (an
# arm64 assembler's ltmpN at the section's start), and of __eh_frame, a
# CIE "CFI" and an FDE "FDE for: " and its function's name.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
static int sfn(void) { return 3; }
const char *str(void) { return "hello"; }
int foo(void) { printf("x\n"); return sfn(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int bar = 5;
int baz(void) { return bar; }
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o -map $t/map

grep -q '^# Path: .*/r.o$' $t/map
grep -q '^\[  0\] linker synthesized$' $t/map
grep -q '^\[  1\] .*/a.o$' $t/map
grep -q '^\[  2\] .*/b.o$' $t/map
grep -q $'^0x00000000\t0x[0-9A-F]*\t__TEXT\t__text$' $t/map
grep -q $'^0x[0-9A-F]*\t0x[0-9A-F]*\t__LD\t__compact_unwind$' $t/map
grep -q $'^0x00000000\t0x[0-9A-F]*\t\[  1\] _str$' $t/map
grep -q $'\t\[  1\] _sfn$' $t/map
grep -q $'\t\[  2\] _baz$' $t/map
grep -q $'\t0x00000006\t\[  1\] literal string: hello$' $t/map
grep -q $'\t0x00000004\t\[  2\] _bar$' $t/map
not grep -q __mh_execute_header $t/map

if [ $ARCH = arm64 ]; then
  grep -Eq $'\t0x00000020\t\\[  1\\] ltmp[0-9]+$' $t/map
  grep -Eq $'\t0x00000020\t\\[  2\\] ltmp[0-9]+$' $t/map
  grep -q $'\t0x00000020\t\[  1\] anon$' $t/map
else
  grep -q $'\t0x00000020\t\[  2\] anon$' $t/map
  grep -q $'\t\[  1\] CFI$' $t/map
  grep -q $'\t\[  2\] FDE for: _baz$' $t/map
fi

# They are written before the output, which may not be writable.
not $mold -r -arch $ARCH -o $t/no/such/r.o $t/a.o -map $t/map2 -dependency_info $t/deps 2> /dev/null
grep -q '^# Path: .*/no/such/r.o$' $t/map2
grep -q a.o $t/deps
