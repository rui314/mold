#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# A word-size absolute relocation whose r_offset lies outside of its
# section must not be applied to whatever happens to follow the section
# in the output file. No compiler emits such an object, so patch the
# offset of a valid one with dd.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl foo
.data
foo:
.quad foo
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
int main() {}
EOF

off=$(readelf -SW $t/a.o | sed 's/\[ *[0-9]*\]//' |
      awk '$1 == ".rela.data" { print $4 }')
[ -n "$off" ] || skip

# r_offset is the first field of the first Elf64_Rela. Point it 256 bytes
# past the 8-byte section.
printf '\0\1\0\0\0\0\0\0' | dd of=$t/a.o bs=1 seek=$((16#$off)) conv=notrunc status=none

not $CC -B. -o $t/exe $t/a.o $t/b.o >& /dev/null
