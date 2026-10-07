#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# The classic weak-bind stream lists, by symbol, every pointer to a weak
# definition dyld may coalesce with another image's: each piece of
# state is set only when it changes, as in the bind stream.
cat <<EOF | $CC -o $t/a.o -c -xassembler - -mmacos-version-min=11.0
.text
.globl _main
.p2align 2
_main:
  ret
.globl _wa
.weak_definition _wa
_wa:
  ret
.globl _wb
.weak_definition _wb
_wb:
  ret
.data
.p2align 3
.globl _ptrs
_ptrs:
  .quad _wa
  .quad _wa
  .quad _wa
  .quad _wb
  .quad 0
  .quad _wa
  .quad _wb
  .quad 0
  .quad 0
  .quad _wb
  .quad _wb
  .quad _wb
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -mmacos-version-min=11.0
ptrs=0x$(nm $t/exe | awk '$3 == "_ptrs" { print $1 }')
objdump --macho --weak-bind $t/exe | awk '$4 == "pointer" { print $3, $5, $6 }' | sort > $t/binds
{
  for off in 0 8 16 40; do printf '0x%x 0 _wa\n' $((ptrs + off)); done
  for off in 24 48 72 80 88; do printf '0x%x 0 _wb\n' $((ptrs + off)); done
} | sort > $t/expected
diff $t/expected $t/binds
[ "$(dyld_info -opcodes $t/exe | grep -c 'SET_SYMBOL_TRAILING_FLAGS_IMM(0x00, _wa)')" -le 1 ]
