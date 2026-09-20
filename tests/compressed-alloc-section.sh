#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# A compressed section is read at its uncompressed size, which its Chdr
# claims, only after it is uncompressed. Allocated sections such as
# .eh_frame are parsed straight from the file, so the gABI forbids
# compressing them. Compilers never do, so build a .eh_frame whose
# contents form a Chdr claiming 1 MiB and mark it SHF_COMPRESSED by
# patching its section header.

cat <<EOF | $CC -c -o $t/a.o -xc -
int main() { return 0; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .eh_frame,"a",@progbits
.long 1, 0
.quad 0x100000
.quad 1
.byte 0x78, 0x9c
.fill 30, 1, 0
EOF

shoff=$(readelf -h $t/b.o | awk '/Start of section headers/ { print $5 }')
idx=$(readelf -SW $t/b.o | sed 's/\[ *\([0-9]*\)\]/\1 /' | awk '$2 == ".eh_frame" { print $1 }')
[ -n "$shoff" -a -n "$idx" ] || skip

# sh_flags is at offset 8 of a 64-byte Elf64_Shdr. Set SHF_ALLOC | SHF_COMPRESSED.
printf '\x02\x08\0\0\0\0\0\0' |
  dd of=$t/b.o bs=1 seek=$((shoff + idx * 64 + 8)) conv=notrunc status=none

not $CC -B. -o $t/exe $t/a.o $t/b.o 2> $t/log
grep 'allocated section is compressed' $t/log
