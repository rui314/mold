#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

not $CC -B. -o $t/exe $t/a.o -Wl,-defsym=foo=bar |&
  grep 'undefined symbol: bar'
