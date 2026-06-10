#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".mydata"))) int d = 3;
int main() { return 0; }
EOF

# .mydata runs at a high address but is stored (loaded) right after
# .text, like a kernel or firmware image
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x100000;
  .text : { *(.text .text.*) }
  . = 0x800000;
  .mydata : AT(ADDR(.mydata) - 0x700000) { *(.mydata) }
  data_lma = LOADADDR(.mydata);
  data_vma = ADDR(.mydata);
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -lW $t/exe > $t/log

# The load segment for .mydata must have PhysAddr = VirtAddr - 0x700000
grep -E 'LOAD .*0x0*800000 0x0*100000' $t/log

readelf -sW $t/exe > $t/log2
grep -E '100000 .* data_lma' $t/log2
grep -E '800000 .* data_vma' $t/log2
