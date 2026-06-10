#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

cat <<'EOF' > $t/script
ENTRY(Reset_Handler)
MEMORY {
  FLASH (rx) : ORIGIN=0x8000000, LENGTH = 1M
  RAM (xrw)  : org = 0x20000000, len = 192K
}
PHDRS {
  text PT_LOAD FILEHDR PHDRS FLAGS(5);
}
SECTIONS {
  __stack_size = DEFINED(__stack_size) ? __stack_size : 4K;
  .isr_vector : { KEEP(*(.isr_vector)) } >FLASH :text
  .text: {
    . = ALIGN(4);
    *(.text .text*)
    *(SORT(.ctors.*))
    LONG(0xdeadbeef)
    SHORT(1 + 2 * 3)
  } >FLASH =0xff
  .data : AT(ADDR(.text) + SIZEOF(.text)) ALIGN(8) {
    *(EXCLUDE_FILE(*crtend.o) .data .data.*)
    libfoo.a:bar.o(.data.hot)
    PROVIDE_HIDDEN(_edata = .);
  } >RAM AT>FLASH
  .noinit (NOLOAD) : { *(.noinit) } >RAM
  /DISCARD/ : { *(.comment) }
  ASSERT(SIZEOF(.isr_vector) == 0x198, "bad vector table")
}
INSERT AFTER .data;
REGION_ALIAS(ROM, FLASH)
EOF

./mold $t/a.o -T $t/script --dump-script -o /dev/null > $t/dump

grep -F 'ENTRY(Reset_Handler)' $t/dump
grep -F 'FLASH (rx) : ORIGIN = 0x8000000, LENGTH = 0x100000' $t/dump
grep -F 'RAM (xrw) : ORIGIN = 0x20000000, LENGTH = 0x30000' $t/dump
grep -F 'text PT_LOAD FILEHDR PHDRS FLAGS(0x5);' $t/dump
grep -F '__stack_size = (DEFINED(__stack_size) ? __stack_size : 0x1000);' $t/dump
grep -F '} >FLASH :text' $t/dump
grep -F '.text : {' $t/dump
grep -F '. = ALIGN(0x4);' $t/dump
grep -F '*(SORT_BY_NAME(.ctors.*))' $t/dump
grep -F 'LONG(0xdeadbeef)' $t/dump
grep -F 'SHORT((0x1 + (0x2 * 0x3)))' $t/dump
grep -F '} >FLASH =0xff' $t/dump
grep -F '.data : AT((ADDR(.text) + SIZEOF(.text))) ALIGN(0x8) {' $t/dump
grep -F '*(EXCLUDE_FILE(*crtend.o) .data .data.*)' $t/dump
grep -F 'libfoo.a:bar.o(.data.hot)' $t/dump
grep -F 'PROVIDE_HIDDEN(_edata = .);' $t/dump
grep -F '} >RAM AT>FLASH' $t/dump
grep -F '.noinit (NOLOAD) : {' $t/dump
grep -F 'ASSERT((SIZEOF(.isr_vector) == 0x198), "bad vector table")' $t/dump
grep -F 'INSERT AFTER .data' $t/dump
grep -F 'REGION_ALIAS(ROM, FLASH)' $t/dump
