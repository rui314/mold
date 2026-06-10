#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

# `. = ASSERT(expr, msg);` after SECTIONS is a common idiom for a
# standalone assertion, e.g. the Linux kernel's KERNEL_IMAGE_SIZE
# check
cat <<'EOF' > $t/script
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  _end = .;
}
. = ASSERT(_end - 0x10000 < 0x100000, "image too big");
EOF

./mold -o $t/exe $t/a.o -T $t/script

cat <<'EOF' > $t/script2
SECTIONS {
  . = 0x10000;
  .text : { *(.text .text.*) }
  _end = .;
}
. = ASSERT(_end - 0x10000 > 0x100000, "this should fail");
EOF

not ./mold -o $t/exe2 $t/a.o -T $t/script2 |& grep 'this should fail'
