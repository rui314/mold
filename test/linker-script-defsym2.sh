#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# A symbol assignment to an integer literal behaves like --defsym
cat <<'EOF' > $t/script
foo = 0x1234;
bar = 256;
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-T,$t/script
readelf -sW $t/exe > $t/log
grep -E '1234 .* ABS foo' $t/log
grep -E '100 .* ABS bar' $t/log
