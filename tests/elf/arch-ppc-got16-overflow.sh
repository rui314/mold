#!/usr/bin/env bash
. $(dirname $0)/common.inc

# -fpic code addresses a GOT slot with a signed 16-bit offset from the
# GOT, so it can't use more than 32 KiB of GOT.
seq 1 5000 | sed 's/.*/lwz 3, v&@got(30)\nlwz 3, t&@got@tprel(30)/' |
  $CC -c -o $t/a.o -xassembler -

not $CC -B. -shared -o $t/b.so $t/a.o 2> $t/log
grep -E 'relocation R_PPC_GOT16 against v[0-9]+ out of range' $t/log
grep -E 'relocation R_PPC_GOT_TPREL16 against t[0-9]+ out of range' $t/log
