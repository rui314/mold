#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.data
.globl abs16, abs32
abs16:
  .2byte foo
.balign 4
abs32:
  .4byte main
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
extern unsigned short abs16;
extern unsigned abs32;
int main() { printf("%x %d\n", abs16, abs32 == (unsigned long)main); }
EOF

$CC -B. -o $t/exe -no-pie $t/a.o $t/b.o -Wl,-defsym=foo=0xfedc
$QEMU $t/exe | grep '^fedc 1$'

not $CC -B. -o $t/exe -pie $t/a.o $t/b.o -Wl,-defsym=foo=0xfedc |&
  grep 'R_AARCH64_ABS32 relocation .* recompile with -fPIC'
