#!/usr/bin/env bash
. $(dirname $0)/common.inc

# An FDE refers to its function with a 32-bit PC-relative relocation, so
# a function that is more than 2 GiB away from .eh_frame cannot be
# described. That must be reported rather than silently wrapped around.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _start, foo
.text
_start:
  ret

.section .foo, "ax"
foo:
  .cfi_startproc
  ret
  .cfi_endproc
EOF

./mold -o $t/exe1 $t/a.o --eh-frame-hdr --section-start=.foo=0x10000000

not ./mold -o $t/exe2 $t/a.o --eh-frame-hdr --section-start=.foo=0x100000000 |&
  grep -E 'relocation R_X86_64_PC32 against .* out of range'
