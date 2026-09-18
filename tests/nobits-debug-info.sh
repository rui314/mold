#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

# A NOBITS section has no bytes in the file, so a NOBITS .debug_info
# reads as zeros. MOLD_DEBUG makes mold read the first bytes of every
# .debug_info to tell DWARF32 from DWARF64.

cat <<EOF | $CC -c -o $t/a.o -xc - -g
int main() { return 0; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .debug_info,"",@nobits
.zero 16
EOF

MOLD_DEBUG=1 $CC -B. -o $t/exe $t/a.o $t/b.o -g
$QEMU $t/exe
