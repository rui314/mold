#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A malformed hexadecimal --defsym value once crashed mold instead of
# being reported as an error.

cat <<EOF | $CC -c -o $t/a.o -xc -
int main() {}
EOF

not $CC -B. -o $t/exe $t/a.o -Wl,-defsym=foo=0xz |&
  grep -- '-defsym: not a number: 0xz'
