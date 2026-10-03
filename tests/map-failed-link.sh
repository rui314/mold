#!/bin/bash
source "$(dirname "$0")"/common.inc

# A link that fails as it writes the output - a relocation it can't
# apply - still writes the reports, the dependency info and the map,
# which come before. (ld-prime writes the dependency info, and the map,
# for a link that fails in its layout too.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__const
.p2align 2
_desc: .long 0
  .long _fm1 - _desc
.section __TEXT,__swift5_fieldmd,regular,no_dead_strip
.p2align 2
_fm1: .long 0
.section __AAA,__a
.quad 0
.section __BBB,__b
.quad 0
.subsections_via_symbols
EOF

reports() { echo "-Wl,-map,$t/$1.map -Wl,-dependency_info,$t/$1.dep"; }

# A reference to Swift metadata the option dropped is an error.
not $CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-remove_swift_reflection_metadata_sections \
  $(reports 1) 2> $t/log1
grep -q "$t/a.o: _desc+0x4: target '_fm1' does not have address" $t/log1
grep -q '^\[  1\] .*/a.o$' $t/1.map
grep -q a.o $t/1.dep
[ ! -e $t/exe1 ]

# Thread-local data a rename moves out of the template is an error in
# the layout.
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
__thread int x = 5;
__thread int y;
int main() { printf("%d %d\n", x, y); }
EOF
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__bar
.long 7
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/c.o $t/b.o \
  -Wl,-rename_section,__DATA,__thread_data,__DATA,__bar $(reports 2) 2> $t/log2
grep -q 'thread-locals too large' $t/log2
[ ! -e $t/exe2 ]

# So is a segment out of order.
not $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-segaddr,__AAA,0x200000000 $(reports 3) 2> $t/log3
grep -q 'segment __BBB address is out of order' $t/log3
[ ! -e $t/exe3 ]
