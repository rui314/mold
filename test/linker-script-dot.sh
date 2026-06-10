#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".dat"), aligned(4))) int d = 7;
int main() { return 0; }
EOF

# Location counter arithmetic in an output section body. The script's
# own assertions verify the layout, including a symbol defined from an
# earlier body symbol.
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  .dat : ALIGN(32) {
    dat_start = .;
    . += 16;
    reserved_end = .;
    *(.dat)
    . = dat_start + 64;
    dat_end = .;
  }
  ASSERT(reserved_end - dat_start == 16, "bad reservation")
  ASSERT(dat_end - dat_start == 64, "bad section size")
  ASSERT(SIZEOF(.dat) == 64, "bad SIZEOF")
  ASSERT(ADDR(.dat) % 32 == 0, "bad alignment")
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -SW $t/exe | grep -E "\.dat .* 000040"

# The input section was placed after the 16-byte reservation
readelf -sW $t/exe > $t/log
dat=$(grep -w dat_start $t/log | awk '{print strtonum("0x" $2)}')
d=$(grep -E " d$" $t/log | grep OBJECT | awk '{print strtonum("0x" $2)}')
test $((d - dat)) = 16
