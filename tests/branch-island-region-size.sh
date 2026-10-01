#!/bin/bash
source "$(dirname "$0")"/common.inc

# -branch_island_region_size spaces ld-prime's branch island clusters.
# Its argument is a hexadecimal number as strtoull() reads it; mold
# places its thunks by each branch's reach, so it only checks it.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

for size in 0x100 1000 ' 1' +5 -1; do
  $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-branch_island_region_size,"$size"
  $t/exe
done

not $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-branch_island_region_size,zz 2> $t/log
grep -q -- '-branch_island_region_size must specify a hexadecimal size' $t/log

not $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-branch_island_region_size,0x 2> $t/log
grep -q -- '-branch_island_region_size must specify a hexadecimal size' $t/log
