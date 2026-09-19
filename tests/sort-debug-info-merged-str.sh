#!/usr/bin/env bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip

# Mixing DWARF32 and DWARF64 debug info makes mold lay out the merged
# .debug_str section in two regions, 32-bit-addressable strings first.
# Shard 0 of the string map once missed the offset adjustment for the
# 64-bit region, so its strings overlapped the 32-bit region.

seq 0 499 | sed 's/.*/void dwarf64_fn_&() {}/' > $t/a.c
$CC -o $t/a.o -c -g -gdwarf64 $t/a.c || skip
seq 0 499 | sed 's/.*/void dwarf32_fn_&() {}/' > $t/b.c
$CC -o $t/b.o -c -g -gdwarf32 $t/b.c

readelf -p .debug_str $t/a.o | grep dwarf64_fn_0 || skip
readelf -p .debug_str $t/b.o | grep dwarf32_fn_0 || skip

echo 'int main() {}' | $CC -o $t/c.o -c -xc - -g

MOLD_DEBUG=1 $CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -g
readelf -p .debug_str $t/exe > $t/log

awk '
  { seen[$NF] = 1 }
  END {
    for (i = 0; i < 500; i++)
      if (!seen["dwarf64_fn_" i] || !seen["dwarf32_fn_" i])
        exit 1
  }
' $t/log
