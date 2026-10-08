#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A relocation in a non-allocated section may refer to a function
# descriptor in .opd. It must be redirected to the function's entry point
# even if it's in a CREL table. GNU as can't emit CREL, so we rewrite a
# RELA table as CREL in the object file.
cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .section .opd, "aw"
  .align 3
  .type foo, @function
foo:
  .quad .L.foo, .TOC.@tocbase, 0

  .text
.L.foo:
  blr

  .section .foo, ""
  .quad foo
EOF

# .rela.foo has one relocation, R_PPC64_ADDR64 against the .opd section
# symbol with addend 0. Overwrite it with a CREL header for one relocation
# with addends, followed by a record with symbol and type deltas.
shoff=$(readelf -h $t/a.o | awk '/Start of section headers/ { print $5 }')
idx=$(readelf -SW $t/a.o | sed 's/\[ *\([0-9]*\)\]/\1 /' | awk '$2 == ".rela.foo" { print $1 }')
off=$(readelf -rW $t/a.o | awk '/\.rela\.foo/ { print $6 }')
info=$(readelf -rW $t/a.o | awk '$3 == "R_PPC64_ADDR64" && $5 == ".opd" { print $2 }')
sym=$((16#${info:0:8}))

printf "\x0c\x03\x$(printf %02x $sym)\x26" |
  dd of=$t/a.o bs=1 seek=$((off)) conv=notrunc status=none

# Set sh_type to SHT_CREL and sh_size to 4. They are at offsets 4 and 32
# of a 64-byte big-endian Elf64_Shdr.
printf '\x40\0\0\x14' |
  dd of=$t/a.o bs=1 seek=$((shoff + idx * 64 + 4)) conv=notrunc status=none
printf '\0\0\0\0\0\0\0\x04' |
  dd of=$t/a.o bs=1 seek=$((shoff + idx * 64 + 32)) conv=notrunc status=none

cat <<EOF | $CC -c -o $t/b.o -xc -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o

addr=$(readelf -sW $t/exe | awk '$4 == "FUNC" && $8 == "foo" { print $2 }')
readelf -x .foo $t/exe | grep -F "${addr:0:8} ${addr:8:8}"
