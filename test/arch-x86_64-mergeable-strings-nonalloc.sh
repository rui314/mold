#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xc -
int main() {}
EOF

cat <<'EOF' | $CC -o $t/b.o -c -x assembler -
.section .foo, "", @progbits
.quad .L1 - 1
.quad .L2 - 1

.section .bar, "MS", @progbits, 1
.L1:
  .string "abc"
.L2:
  .string "xyz"
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o

readelf -x .foo $t/exe | grep -Fq '03000000 00000000 ffffffff ffffffff'
readelf -x .bar $t/exe | grep -Fq 'xyz.abc.'
