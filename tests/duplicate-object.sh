#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime loads an object file as often as the command line names it:
# twice over, its globals are duplicate definitions and its local data
# is there twice, and -t lists it twice. A library loads once.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
cat <<EOF | $CC -o $t/b.o -c -xc -
static int y[4] __attribute__((used)) = {5, 6, 7, 8};
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/a.o 2> $t/log
grep -q 'duplicate symbol' $t/log
grep -q _main $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/b.o -Wl,-t > $t/log2
[ "$(size -m $t/exe | awk '$2 == "__data:" { print $3 }')" = 32 ]
[ "$(grep -c '/b.o$' $t/log2)" = 2 ]

rm -f $t/c.a
ar rcs $t/c.a $t/b.o
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-force_load,$t/c.a,-force_load,$t/c.a \
  -Wl,-no_warn_duplicate_libraries
[ "$(size -m $t/exe | awk '$2 == "__data:" { print $3 }')" = 16 ]
