#!/bin/bash
source "$(dirname "$0")"/common.inc

# clang passes its -O level on to the linker as given (-O2, -Os, -Ofast,
# -Og, -O4, ...; -O and -O1 as "-no_deduplicate -O1"), and ld-prime takes
# -O followed by anything. It uses the level only to skip deduplication
# of identical functions at -O0 (or with no -O at all); mold ignores the
# level and deduplicates unless -no_deduplicate, so here all the levels
# give the same output (under ld-prime they don't).
cat <<EOF | $CXX -o $t/a.o -c -xc++ - -O2
template <int N> __attribute__((noinline)) int f(int x) { return x * 3 + 7; }
int main(int argc, char **) { return f<1>(argc) + f<2>(argc) != argc * 6 + 14; }
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
$RUN $t/exe
cp $t/exe $t/exe.none
for opt in -O0 -O1 -O2 -O3 -Os -Oz -O -O4 -Ofast -Og -Ofoo; do
  $CXX --ld-path=$mold -o $t/exe $t/a.o -Wl,$opt
  cmp $t/exe.none $t/exe
done
