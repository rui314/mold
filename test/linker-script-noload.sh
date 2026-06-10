#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".reserve"))) int r = 1;
__attribute__((section(".meta"))) int m = 2;
int main() { return 0; }
EOF

cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .reserve (NOLOAD) : { *(.reserve) }
  .meta (INFO) : { *(.meta) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -SW $t/exe > $t/log

# A NOLOAD section becomes NOBITS; an INFO section loses SHF_ALLOC
grep -F .reserve $t/log | grep -w NOBITS
grep -F .meta $t/log | grep -vw A | grep -w PROGBITS
