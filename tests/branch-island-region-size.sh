#!/bin/bash
source "$(dirname "$0")"/common.inc

# -branch_island_region_size spaces ld-prime's branch island clusters.
# Its argument is a hexadecimal number; mold places its thunks by each
# branch's reach, so it only checks it.
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

for size in 0x100 0X100 1000; do
  $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-branch_island_region_size,"$size"
  $t/exe
done

for size in zz 0x 0x0x10; do
  not $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-branch_island_region_size,$size 2> $t/log
  grep -q -- '-branch_island_region_size must specify a hexadecimal size' $t/log
done
