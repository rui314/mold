#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section(".foo"))) int foo_var = 1;
__attribute__((section(".bar.x"))) int bar_var = 2;
__attribute__((section(".keep"))) int keep_var = 3;
int main() { return 0; }
EOF

cat <<'EOF' > $t/script
SECTIONS {
  /DISCARD/ : { *(.foo) *(.bar.*) }
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-T,$t/script
readelf -SW $t/exe1 > $t/log1
not grep -F .foo $t/log1
not grep -F .bar $t/log1
grep -F .keep $t/log1

# EXCLUDE_FILE and per-file patterns restrict what is discarded
cat <<'EOF' > $t/script2
SECTIONS {
  /DISCARD/ : { *(EXCLUDE_FILE(*a.o) .foo) no-such-file.o(.bar.*) }
}
EOF

$CC -B. -o $t/exe2 $t/a.o -Wl,-T,$t/script2
readelf -SW $t/exe2 > $t/log2
grep -F .foo $t/log2
grep -F .bar.x $t/log2

# A /DISCARD/ with output section attributes is not a plain discard
# and must be rejected
cat <<'EOF' > $t/script3
SECTIONS {
  /DISCARD/ 0x1000 : { *(.foo) }
}
EOF

not $CC -B. -o $t/exe3 $t/a.o -Wl,-T,$t/script3 |& grep 'cannot have output section attributes'
