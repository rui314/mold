#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.subsections_via_symbols
.globl _fn1, _fn2
.text
.align 4
_fn1:
  nop
_fn2:
  nop
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _fn3, _fn4
.text
.align 16
_fn3:
  .byte 0xcc
_fn4:
  .byte 0xcc
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

void fn1();
void fn2();
void fn3();
void fn4();

int main() {
  printf("%lu %lu\n", (char *)fn2 - (char *)fn1, (char *)fn4 - (char *)fn3);
}
EOF

# _fn2 sits 4 bytes into a 16-aligned section; ld64 keeps it at
# 4 mod 16 in the output (an atom keeps its offset modulo its section's
# alignment), so the two functions stay 4 bytes apart - not 16, which
# rounding each atom up to the section alignment would give. (This is
# sold's test, whose expectation of 16 came from that rounding.)
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o 2> $t/log
# b.o's .align 16 asks for a 64KB-aligned __text, beyond the page;
# ld64 reduces it with a warning, and dyld_info rejects an unreduced one.
grep -q 'reducing alignment of section __TEXT,__text' $t/log
dyld_info -exports $t/exe | grep _main
if [ $ARCH = arm64 ]; then
  $t/exe | grep '^4 1$'
else
  $t/exe | grep '^1 1$'   # a one-byte nop
fi
