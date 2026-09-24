#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Truncated .eh_frame records must be reported as warnings and dropped, not
# crash the linker.

# A record whose length field promises fewer bytes than the CIE id needs.
cat <<EOF | $CC -c -o $t/a.o -xassembler -
.section .eh_frame,"a"
  .byte 0x03
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
EOF

$CC -B. -shared -o $t/a.so $t/a.o 2> $t/a.log
grep -q 'corrupted .eh_frame' $t/a.log

# A record whose length extends past the end of the section.
cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .eh_frame,"a"
  .byte 0x08
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x01
  .byte 0x01
EOF

$CC -B. -shared -o $t/b.so $t/b.o 2> $t/b.log
grep -q 'corrupted .eh_frame' $t/b.log

# A CIE that fits in the section but is too small to be parsed.
cat <<EOF | $CC -c -o $t/c.o -xassembler -
.section .eh_frame,"a"
  .byte 0x05
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x00
  .byte 0x01
EOF

$CC -B. -shared -o $t/c.so $t/c.o 2> $t/c.log
grep -q 'corrupted .eh_frame' $t/c.log
