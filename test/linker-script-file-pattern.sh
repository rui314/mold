#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".dat"))) char a = 1;
int main() { return 0; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
__attribute__((section(".dat"))) char b = 2;
EOF

# Input sections can be selected by the file they come from
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .first : { *b.o(.dat) }
  .rest : { *(.dat) }
}
EOF

./mold -o $t/exe $t/a.o $t/b.o -T $t/script
readelf -SW $t/exe > $t/log
grep -F .first $t/log
grep -F .rest $t/log

off=$(readelf -SW $t/exe | grep -F .first | awk '{print strtonum("0x" $6)}')
test "$(dd if=$t/exe bs=1 skip=$off count=1 2>/dev/null | od -An -tx1 | tr -d ' ')" = 02
