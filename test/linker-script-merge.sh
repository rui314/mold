#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -O2 -o $t/a.o -c -xc -
const char *f() { return "hello mold script"; }
EOF
cat <<EOF | $CC -O2 -o $t/b.o -c -xc -
const char *g() { return "hello mold script"; }
int main() { return 0; }
EOF

# Identical string literals from different files must be merged into
# a single copy, placed inside the output section the script names
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  .rodata : { ro_start = .; *(.rodata .rodata.*) ro_end = .; }
  ASSERT(ro_end - ro_start > 0, "rodata must not be empty")
}
EOF

./mold -o $t/exe $t/a.o $t/b.o -T $t/script
test $(readelf -p .rodata $t/exe | grep -c 'hello mold script') = 1

# The merged strings lie within the script's section bounds
readelf -sW $t/exe > $t/log
ro_start=$(grep -w ro_start $t/log | awk '{print strtonum("0x" $2)}')
ro_end=$(grep -w ro_end $t/log | awk '{print strtonum("0x" $2)}')
size=$(readelf -SW $t/exe | grep -F " .rodata " | sed 's/.*PROGBITS//' | \
  awk '{print strtonum("0x" $3)}')
test $((ro_end - ro_start)) = $size
