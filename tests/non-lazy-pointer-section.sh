#!/bin/bash
source "$(dirname "$0")"/common.inc

# 32-bit code named the targets of its pointer slots in the indirect
# symbol table (.indirect_symbol in a non_lazy_symbol_pointers section)
# rather than by relocations. ld-prime rejects such a section, whose
# slots would otherwise be left null - by its type, whatever its name,
# and in an object with indirect symbols even an empty one or one the
# table names no slot of.
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

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _main
.p2align 2
_main:
  ret
.section __DATA,__lazy,lazy_symbol_pointers
.p2align 3
.indirect_symbol _main
.quad 0
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
EOF
not $CC --ld-path=$mold -o $t/exe $t/b.o 2> $t/log
grep -qF "non-lazy pointers sections no longer supported for 64-bit architectures in '$t/b.o'" $t/log

# Without indirect symbols, the section's relocations fill it.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
.p2align 3
.quad _foo
.text
.globl _foo
_foo:
  ret
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o
$t/exe
