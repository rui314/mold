#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -x assembler -c -o $t/a.o -Wa,-L -
.globl _start, foo
_start:
foo:
bar:
.L.baz:
EOF

./mold -o $t/exe $t/a.o
readelf --symbols $t/exe > $t/log
grep -F _start $t/log
grep -F foo $t/log
grep -F bar $t/log
not grep -F .L.baz $t/log

./mold -o $t/exe $t/a.o --discard-none
readelf --symbols $t/exe > $t/log
grep -F _start $t/log
grep -F foo $t/log
grep -F bar $t/log
grep -F .L.baz $t/log

./mold -o $t/exe $t/a.o -strip-all
readelf --symbols $t/exe > $t/log
not grep -F _start $t/log
not grep -F foo $t/log
not grep -F bar $t/log
not grep -F .L.baz $t/log
