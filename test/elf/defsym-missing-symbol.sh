#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

! $CC -B. -o $t/exe $t/a.o -Wl,-defsym=foo=bar 2> $t/log
grep -q 'undefined symbol: bar' $t/log
