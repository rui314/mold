#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -ffunction-sections
__attribute__((section(".keepme"))) int keep_var = 1;
__attribute__((section(".dropme"))) int drop_var = 2;
int main() { return 0; }
EOF

cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .keepme : { KEEP(*(.keepme)) }
  .dropme : { *(.dropme) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script --gc-sections -e main
readelf -SW $t/exe > $t/log

# KEEP saves an otherwise unreferenced section from --gc-sections
grep -F .keepme $t/log
not grep -F .dropme $t/log
