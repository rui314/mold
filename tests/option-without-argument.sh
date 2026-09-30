#!/usr/bin/env bash
. $(dirname $0)/common.inc

# These options take no argument, so the file following each of them
# is an input file.

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 -Wl,--dynamic-list-data $t/a.o
$CC -B. -o $t/exe2 -Wl,--thinlto-index-only $t/a.o
$CC -B. -o $t/exe3 -Wl,--lto-pseudo-probe-for-profiling $t/a.o

# GNU ld's --package-metadata takes an optional argument, which must
# therefore be attached with an equal sign.
not $CC -B. -o $t/exe4 -Wl,--package-metadata $t/a.o |&
  grep 'unknown command line option: --package-metadata'
