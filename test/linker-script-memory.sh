#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".text.main"))) int main() { return 0; }
__attribute__((section(".rodata.str"))) const char ro[16] = "read-only";
__attribute__((section(".data.var"))) int var = 7;
__attribute__((section(".bss.var"))) int zero;
EOF

# The classic microcontroller layout: code and constants in flash,
# data in RAM with its initialization image stacked in flash
cat <<'EOF' > $t/script
MEMORY {
  flash (rx)  : ORIGIN = 0x8000000, LENGTH = 128K
  ram   (rwx) : ORIGIN = 0x20000000, LENGTH = 64K
}
REGION_ALIAS("rom", flash);
SECTIONS {
  .text   : { *(.text .text.*) } >rom
  .rodata : { *(.rodata .rodata.*) } >flash
  .data   : { *(.data .data.*) } >ram AT>flash
  .bss    : { *(.bss .bss.*) } >ram
  data_lma = LOADADDR(.data);
  flash_end = ORIGIN(flash) + LENGTH(flash);
  ASSERT(data_lma + SIZEOF(.data) <= flash_end, "flash overflow")
}
EOF

./mold -o $t/exe $t/a.o -T $t/script

addr_of() {
  readelf -SW $1 | grep -F " $2 " | sed 's/.*BITS//' | \
    awk '{print strtonum("0x" $1)}'
}

in_range() {
  test $1 -ge $(($2)) && test $1 -lt $(($3))
}

# Code and constants are in flash, data and bss in RAM
test $(addr_of $t/exe .text) = $((0x8000000))
in_range $(addr_of $t/exe .rodata) 0x8000000 0x8020000
in_range $(addr_of $t/exe .data) 0x20000000 0x20010000
in_range $(addr_of $t/exe .bss) 0x20000000 0x20010000

# .data's load image is in flash, after the read-only sections
readelf -sW $t/exe > $t/log2
rodata_end=$(readelf -SW $t/exe | grep -F " .rodata " | sed 's/.*PROGBITS//' | \
  awk '{print strtonum("0x" $1) + strtonum("0x" $3)}')
lma=$(grep -w data_lma $t/log2 | awk '{print strtonum("0x" $2)}')
test $lma -ge $rodata_end
test $lma -lt $((0x8020000))

readelf -lW $t/exe | grep -E 'LOAD .*0x0*2000[0-9a-f]{4} 0x0*8[0-9a-f]{6} .* RW'

# Overflowing a region must be reported
cat <<'EOF' > $t/script2
MEMORY {
  flash (rx)  : ORIGIN = 0x8000000, LENGTH = 4
  ram   (rw)  : ORIGIN = 0x20000000, LENGTH = 64K
}
SECTIONS { .text : { *(.text .text.*) } >flash }
EOF

not ./mold -o $t/exe2 $t/a.o -T $t/script2 |& \
  grep 'overflows memory region flash'

# A section that names no region is placed by region attributes
cat <<'EOF' > $t/script3
MEMORY {
  flash (rx)  : ORIGIN = 0x8000000, LENGTH = 128K
  ram   (rw)  : ORIGIN = 0x20000000, LENGTH = 64K
}
SECTIONS {
  .text : { *(.text .text.*) }
  .data : { *(.data .data.*) }
  .bss  : { *(.bss .bss.*) }
}
EOF

./mold -o $t/exe3 $t/a.o -T $t/script3
test $(addr_of $t/exe3 .text) = $((0x8000000))
in_range $(addr_of $t/exe3 .data) 0x20000000 0x20010000
