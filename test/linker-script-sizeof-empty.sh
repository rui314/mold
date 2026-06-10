#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC -
int main() { return 0; }
EOF

# SIZEOF/ADDR of a section the script declares but that doesn't
# materialize evaluate to zero. Our always-created .got placeholder
# must not be claimed by the script's .got description, so the Linux
# kernel's GOT assertions hold in a static link.
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .got : { *(.got) }
  got_size = SIZEOF(.got);
  nowhere = ADDR(.got);
  ASSERT(SIZEOF(.got) == 0, "expected no GOT input sections")
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
readelf -sW $t/exe > $t/log
grep -E ' 0 .* ABS got_size' $t/log || \
  grep -E '^.* 0{8,16} .* got_size' $t/log
