#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.data
.globl abs8, abs16
abs8:
  .byte foo
.balign 2
abs16:
  .short bar
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
extern unsigned char abs8;
extern unsigned short abs16;
int main() { printf("%x %x\n", abs8, abs16); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-defsym=foo=0xab,-defsym=bar=0xfedc
$QEMU $t/exe | grep '^ab fedc$'

cat <<EOF | $CC -c -o $t/c.o -xassembler -
.data
.short main
EOF

not $CC -B. -o $t/exe -pie $t/a.o $t/b.o $t/c.o \
  -Wl,-defsym=foo=0xab,-defsym=bar=0xfedc |&
  grep 'R_ARM_ABS16 relocation .* recompile with -fPIC'
