#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

# Sections that never materialize: their body symbols still get
# addresses, the location counter still moves, and SIZEOF/ADDR of
# such sections read as zero so assertions about them work.
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  . = 0x20000;
  .empty1 : { empty1_start = .; KEEP(*(.no_such_input)) empty1_end = .; }
  .reserve : { . += 0x100; }
  after_reserve = .;
  ASSERT(SIZEOF(.empty1) == 0, "SIZEOF of an empty section must be 0")
  ASSERT(after_reserve == 0x20100, "the reservation must move the dot")
  .data : { *(.data .data.*) }
  .bss : { *(.bss .bss.*) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -sW $t/exe > $t/log
grep -E '20000 .* empty1_start' $t/log
grep -E '20000 .* empty1_end' $t/log
grep -E '20100 .* after_reserve' $t/log
