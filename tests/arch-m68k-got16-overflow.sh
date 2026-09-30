#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A 16-bit GOT offset is a signed displacement, so it can address only
# the first 32 KiB of the GOT.

cat <<'EOF' | $CC -c -o $t/a.o -xassembler -
.macro ref
  move.l (v\@@GOT.w, %a5), %d0
.endm
.rept 8200
  ref
.endr
EOF

not ./mold -shared -o $t/b.so $t/a.o |&
  grep -E 'relocation R_68K_GOTOFF16 against v[0-9]+ out of range: 32768 '
