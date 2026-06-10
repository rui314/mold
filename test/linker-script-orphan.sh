#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".orphan"))) int o = 1;
int main() { return 0; }
EOF

cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .data : { *(.data .data.*) }
}
EOF

# An orphan section is placed even though the script doesn't mention it
./mold -o $t/exe $t/a.o -T $t/script
readelf -SW $t/exe | grep -F .orphan

./mold -o $t/exe $t/a.o -T $t/script --orphan-handling=warn |& grep 'placing orphan section .orphan'
not ./mold -o $t/exe2 $t/a.o -T $t/script --orphan-handling=error |& grep 'orphan section .orphan'
