#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

# The gap created by moving the location counter is written with the
# fill pattern
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .pad : {
    BYTE(0x11)
    . += 15;
    BYTE(0x22)
  } =0xcafe01aa
}
EOF

./mold -o $t/exe $t/a.o -T $t/script

off=$(readelf -SW $t/exe | grep -F " .pad" | awk '{print strtonum("0x" $6)}')
bytes=$(dd if=$t/exe bs=1 skip=$off count=17 2>/dev/null | od -An -tx1 | tr -d ' \n')
# member byte, then 15 fill bytes (the big-endian pattern restarts at
# each gap), then the second byte
test "$bytes" = 11cafe01aacafe01aacafe01aacafe0122
