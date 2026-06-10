#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

# A section made only of data commands, padded with a fill pattern.
# The script's own ASSERTs check the layout.
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  .blob : ALIGN(16) {
    blob_start = .;
    BYTE(0x12)
    SHORT(0x3456)
    LONG(0x789abcde)
    QUAD(0x1122334455667788)
    . = ALIGN(16);
    blob_end = .;
  } =0xaaaaaaaa
  ASSERT(blob_end - blob_start == 16, "bad blob size")
}
EOF

./mold -o $t/exe $t/a.o -T $t/script

off=$(readelf -SW $t/exe | grep -F .blob | awk '{print strtonum("0x" $6)}')
test "$(dd if=$t/exe bs=1 skip=$off count=1 2>/dev/null | od -An -tx1 | tr -d ' ')" = 12
# The padding byte at offset 15 comes from the fill pattern, which
# starts anew at each gap
test "$(dd if=$t/exe bs=1 skip=$((off + 15)) count=1 2>/dev/null | od -An -tx1 | tr -d ' ')" = aa
