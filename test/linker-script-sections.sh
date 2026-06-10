#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".foo..bar"))) int a = 1;
__attribute__((section(".foo.x"))) int b = 2;
__attribute__((section(".foo"))) int c = 3;
int main() { return 0; }
EOF

# The first matching pattern in the script wins: .foo..bar must go to
# the first output section even though *(.foo.*) also matches it.
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .foo..bar : { *(.foo..bar) }
  .foo : { *(.foo) *(.foo.*) }
  .text : { *(.text .text.*) }
  .data : { *(.data .data.*) }
  .bss : { *(.bss .bss.*) }
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/script -no-pie -nostdlib
readelf -SW $t/exe > $t/log

grep -F .foo..bar $t/log
test $(readelf -SW $t/exe | grep -F " .foo" | wc -l) = 2
grep -E "\.foo\.\.bar.*000004" $t/log
grep -E " \.foo .*000008" $t/log

# Sections appear in script order at script addresses
adr1=$(readelf -SW $t/exe | grep -F ".foo..bar" | awk '{print $5}' | sed 's/^0*//')
test "$adr1" = 10000
