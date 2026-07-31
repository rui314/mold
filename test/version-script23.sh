#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' > $t/a.ver
VER0 { **; };
VER1 { foo\?; };
VER2 { *foo\bar*; };
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler - >& /dev/null || skip
.globl all_stars, "foo?", xfoobarx
all_stars:
"foo?":
xfoobarx:
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o
readelf -W --dyn-syms $t/c.so > $t/log
grep -F 'all_stars@@VER0' $t/log
grep -F 'foo?@@VER1' $t/log
grep -F 'xfoobarx@@VER2' $t/log
