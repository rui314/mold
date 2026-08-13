#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A relocation against a named local symbol must refer to the local's entry
# in the output .symtab with --emit-relocs. We once computed its symbol index
# against the global part of the symbol table, so the emitted relocation
# referred to an arbitrary unrelated global symbol.
# https://github.com/rui314/mold/issues/1631
#
# GAS usually redirects relocations against local symbols to section
# symbols; we use .reloc to bypass that.

echo | $CC -c -o $t/n.o -xc -
if readelf -h $t/n.o | grep -q ELF64; then
  reloc=BFD_RELOC_64
else
  reloc=BFD_RELOC_32
fi

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.text
.globl _start
_start:
  .reloc _start, $reloc, str
  .space 8

.data
str:
  .string "Hello"
EOF

$CC -B. -o $t/exe $t/a.o -nostdlib -no-pie -Wl,--emit-relocs
readelf -rW $t/exe > $t/log
grep -w str $t/log
