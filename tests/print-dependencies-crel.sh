#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# --print-dependencies reads relocations of all sections, including
# non-allocated ones whose CREL tables aren't decoded into arrays. GNU as
# can't emit CREL, so we rewrite a RELA table as CREL in the object file.
cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .section .foo, ""
  .quad bar
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
int bar;
int main() {}
EOF

# .rela.foo has one relocation, R_X86_64_64 against bar with addend 0.
# Overwrite it with a CREL header for one relocation with addends,
# followed by a record with symbol and type deltas.
shoff=$(readelf -h $t/a.o | awk '/Start of section headers/ { print $5 }')
idx=$(readelf -SW $t/a.o | sed 's/\[ *\([0-9]*\)\]/\1 /' | awk '$2 == ".rela.foo" { print $1 }')
off=$(readelf -rW $t/a.o | awk '/\.rela\.foo/ { print $6 }')
info=$(readelf -rW $t/a.o | awk '$3 == "R_X86_64_64" && $5 == "bar" { print $2 }')
sym=$((16#${info:0:8}))

printf "\x0c\x03\x$(printf %02x $sym)\x01" |
  dd of=$t/a.o bs=1 seek=$((off)) conv=notrunc status=none

# Set sh_type to SHT_CREL and sh_size to 4. They are at offsets 4 and 32
# of a 64-byte little-endian Elf64_Shdr.
printf '\x14\0\0\x40' |
  dd of=$t/a.o bs=1 seek=$((shoff + idx * 64 + 4)) conv=notrunc status=none
printf '\x04\0\0\0\0\0\0\0' |
  dd of=$t/a.o bs=1 seek=$((shoff + idx * 64 + 32)) conv=notrunc status=none

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--print-dependencies > $t/log
grep 'a\.o:(\.foo).*b\.o.*bar$' $t/log
