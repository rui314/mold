#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

! $CC -B. -o $t/exe $t/a.o -Wl,--defsym=foo=bar >& $t/log
grep -Fq 'defsym: undefined symbol: bar' $t/log
