#!/bin/bash
source "$(dirname "$0")"/common.inc

# 32-bit code named the targets of its pointer slots in the indirect
# symbol table (.indirect_symbol in a non_lazy_symbol_pointers section)
# rather than by relocations. ld-prime rejects such a section, whose
# slots would otherwise be left null.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main
.p2align 2
_main:
  ret
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
.p2align 3
.indirect_symbol _main
.quad 0
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
grep -q "non-lazy pointers sections no longer supported for 64-bit architectures in '.*a.o'" $t/log
not $mold -arch $ARCH -r -o $t/r.o $t/a.o 2> $t/log2
grep -q 'non-lazy pointers sections no longer supported' $t/log2
