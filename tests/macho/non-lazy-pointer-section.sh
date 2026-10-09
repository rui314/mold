#!/bin/bash
source "$(dirname "$0")"/common.inc

# 32-bit code named the targets of its pointer slots in the indirect
# symbol table (.indirect_symbol in a section of non-lazy or lazy symbol
# pointers) rather than by relocations. The linker reads relocations
# alone and would leave such slots null, so an object with indirect
# symbols and such a section fails the link, -r too, whatever the
# section's name and even if it is empty.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _foo
_foo: ret
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> /dev/null
not $mold -arch $ARCH -r -o $t/r.o $t/a.o 2> /dev/null

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _foo
_foo: ret
.section __DATA,__la_symbol_ptr,lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/b.o 2> /dev/null
not $mold -arch $ARCH -r -o $t/r.o $t/b.o 2> /dev/null

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _foo
_foo: ret
.section __DATA,__lazy,lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o 2> /dev/null

# ld-prime refuses lazy pointers only in a section named __la_symbol_ptr
# and leaves those of another name null.
if is_mold; then
  cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _foo
_foo: ret
.section __DATA,__lazy,lazy_symbol_pointers
.p2align 3
.indirect_symbol _foo
.quad 0
EOF
  not $CC --ld-path=$mold -o $t/exe $t/main.o $t/d.o 2> /dev/null
fi

# Without indirect symbols, the section's relocations fill it.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __DATA,__nl_symbol_ptr,non_lazy_symbol_pointers
.p2align 3
.quad _foo
.text
.globl _foo
_foo:
  ret
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/e.o
$RUN $t/exe
