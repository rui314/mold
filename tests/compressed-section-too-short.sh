#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# A compressed section starts with a Chdr. Compilers never emit one that
# is shorter, so build a four-byte section and mark it SHF_COMPRESSED by
# patching its section header.

cat <<EOF | $CC -c -o $t/a.o -xc -
int main() { return 0; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .foo,"",@progbits
.long 0
EOF

shoff=$(readelf -h $t/b.o | awk '/Start of section headers/ { print $5 }')
idx=$(readelf -SW $t/b.o | sed 's/\[ *\([0-9]*\)\]/\1 /' | awk '$2 == ".foo" { print $1 }')
[ -n "$shoff" -a -n "$idx" ] || skip

# sh_flags is at offset 8 of a 64-byte Elf64_Shdr. Set SHF_COMPRESSED.
printf '\0\x08\0\0\0\0\0\0' |
  dd of=$t/b.o bs=1 seek=$((shoff + idx * 64 + 8)) conv=notrunc status=none

not $CC -B. -o $t/exe $t/a.o $t/b.o 2> $t/log
grep 'corrupted compressed section' $t/log
