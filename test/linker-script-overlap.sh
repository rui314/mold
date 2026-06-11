#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".lo"))) int lo = 1;
int main() { return 0; }
EOF

# Sections may be given addresses in any order
cat <<'EOF' > $t/script
SECTIONS {
  .text 0x20000 : { *(.text .text.*) }
  .lo 0x10000 : { *(.lo) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -SW $t/exe > $t/log
grep -E '\.text .* 0*20000 ' $t/log
grep -E '\.lo .* 0*10000 ' $t/log

# Loadable segments must still be sorted by address
readelf -lW $t/exe | grep LOAD | head -1 | grep -E '0x0*10000 '

# Overlapping sections are an error
cat <<'EOF' > $t/script2
SECTIONS {
  .text 0x20000 : { *(.text .text.*) }
  .lo 0x20004 : { *(.lo) }
}
EOF

not ./mold -o $t/exe2 $t/a.o -T $t/script2 |& grep 'overlaps with .text'
