#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".tbl.20"))) char t20 = 20;
__attribute__((section(".tbl.3"))) char t3 = 3;
int main() { return 0; }
EOF

# The Linux kernel idiom: KEEP'ed, sorted table sections
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .tbl : {
    tbl_start = .;
    KEEP(*(SORT_BY_INIT_PRIORITY(.tbl.*)))
    tbl_end = .;
  }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script --gc-sections -e main
readelf -sW $t/exe > $t/log

# Both survive gc, in numeric order
a3=$(grep -E " t3$" $t/log | awk '{print strtonum("0x" $2)}')
a20=$(grep -E " t20$" $t/log | awk '{print strtonum("0x" $2)}')
test $a3 -lt $a20
