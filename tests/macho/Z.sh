#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

# With -Z, -lSystem is no longer found in the SDK's /usr/lib.
not $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-Z 2> $t/log
grep -q 'library not found: -lSystem' $t/log
