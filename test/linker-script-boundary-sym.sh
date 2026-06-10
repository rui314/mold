#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".aa"))) int a = 1;
__attribute__((section(".bb"))) int b = 2;
int main() { return 0; }
EOF

# end_aa lies exactly on the boundary of .aa and .bb. It must be
# associated with the preceding section, as in GNU ld; tools strip
# sections (e.g. objcopy --remove-section) together with the symbols
# they contain.
cat <<'EOF' > $t/script
SECTIONS {
  .text : { *(.text .text.*) }
  .aa : { *(.aa) }
  end_aa = .;
  .bb : { *(.bb) }
}
EOF

./mold -o $t/exe $t/a.o -T $t/script
idx=$(readelf -SW $t/exe | grep -F " .aa" | sed 's/.*\[ *\([0-9]*\)\].*/\1/')
readelf -sW $t/exe | grep -w end_aa | grep -w $idx
