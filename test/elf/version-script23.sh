#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER1 { foo\?; };
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.globl "foo?"
"foo?":
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -Fq 'foo?@@VER1' $t/log
