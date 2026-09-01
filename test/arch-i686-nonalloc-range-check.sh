#!/usr/bin/env bash
. $(dirname $0)/common.inc

# We once passed arguments to the relocation range checker in the wrong
# order in non-alloc sections, so that the relocation index instead of the
# relocation value was range-checked. Test both directions: many in-range
# relocations must not be reported, and an out-of-range value must be.

cat <<'EOF' | $CC -c -o $t/a.o -xassembler -
.globl main
main:
  ret

.section .nonalloc
.rept 300
.byte small
.endr
EOF

$CC -B. -o $t/exe1 $t/a.o -no-pie -Wl,-defsym=small=16

cat <<'EOF' | $CC -c -o $t/b.o -xassembler -
.globl main
main:
  ret

.section .nonalloc
.byte main
EOF

not $CC -B. -o $t/exe2 $t/b.o -no-pie |& grep 'out of range'
