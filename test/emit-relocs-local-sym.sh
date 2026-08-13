#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A relocation against a named local symbol must refer to the local's entry
# in the output .symtab with --emit-relocs. We once computed its symbol index
# against the global part of the symbol table, so the emitted relocation
# referred to an arbitrary unrelated global symbol.
# https://github.com/rui314/mold/issues/1631
#
# Assemblers usually redirect relocations against local symbols to section
# symbols, so we assemble `str` as a global symbol and localize it with
# objcopy afterwards.

echo | $CC -c -o $t/a.o -xc -
if readelf -h $t/a.o | grep -q ELF64; then
  reloc=BFD_RELOC_64
else
  reloc=BFD_RELOC_32
fi

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.text
.globl _start
_start:
  .reloc _start, $reloc, str
  .space 8

.data
.globl str
str:
  .string "Hello"
EOF

$OBJCOPY --localize-symbol=str $t/b.o

$CC -B. -o $t/exe $t/b.o -nostdlib -no-pie -Wl,--emit-relocs
readelf -rW $t/exe | grep -w str
