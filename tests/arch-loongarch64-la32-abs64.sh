#!/usr/bin/env bash
. $(dirname $0)/common.inc

# `.dword sym` is assembled into R_LARCH_64 even in an LA32 object. That
# is not the word-size absolute relocation on LA32, so it cannot be
# turned into a dynamic relocation and must be resolved at link-time.

cat <<EOF | clang --target=loongarch32-linux-gnu -o $t/a.o -c -xassembler - || skip
.globl _start
_start:
  ret
.data
.dword _start
EOF

./mold -o $t/exe1 $t/a.o --section-start=.text=0x12345678
readelf -x .data $t/exe1 | grep -F '78563412 00000000'

not ./mold -pie -o $t/exe2 $t/a.o |&
  grep 'R_LARCH_64 relocation .* can not be used; recompile with -fPIC'
