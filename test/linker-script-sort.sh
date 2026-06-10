#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".x.10"))) char c10 = 10;
__attribute__((section(".x.2"))) char c2 = 2;
__attribute__((section(".x.1"))) char c1 = 1;
int main() { return 0; }
EOF

# SORT_BY_NAME is lexicographic: .x.1 .x.10 .x.2
cat <<'EOF' > $t/script1
SECTIONS {
  .text : { *(.text .text.*) }
  .x : { *(SORT_BY_NAME(.x.*)) }
}
EOF

./mold -o $t/exe1 $t/a.o -T $t/script1
readelf -sW $t/exe1 > $t/log1
a1=$(grep -E " c1$" $t/log1 | awk '{print strtonum("0x" $2)}')
a2=$(grep -E " c2$" $t/log1 | awk '{print strtonum("0x" $2)}')
a10=$(grep -E " c10$" $t/log1 | awk '{print strtonum("0x" $2)}')
test $a1 -lt $a10 && test $a10 -lt $a2

# SORT_BY_INIT_PRIORITY is numeric: .x.1 .x.2 .x.10
cat <<'EOF' > $t/script2
SECTIONS {
  .text : { *(.text .text.*) }
  .x : { *(SORT_BY_INIT_PRIORITY(.x.*)) }
}
EOF

./mold -o $t/exe2 $t/a.o -T $t/script2
readelf -sW $t/exe2 > $t/log2
a1=$(grep -E " c1$" $t/log2 | awk '{print strtonum("0x" $2)}')
a2=$(grep -E " c2$" $t/log2 | awk '{print strtonum("0x" $2)}')
a10=$(grep -E " c10$" $t/log2 | awk '{print strtonum("0x" $2)}')
test $a1 -lt $a2 && test $a2 -lt $a10

# SORT_NONE keeps the input order
cat <<'EOF2' > $t/script3
SECTIONS {
  .text : { *(.text .text.*) }
  .x : { *(SORT_NONE(.x.*)) }
}
EOF2

./mold -o $t/exe3 $t/a.o -T $t/script3
readelf -sW $t/exe3 > $t/log3
a2=$(grep -E " c2$" $t/log3 | awk '{print strtonum("0x" $2)}')
a10=$(grep -E " c10$" $t/log3 | awk '{print strtonum("0x" $2)}')
test $a10 -lt $a2
