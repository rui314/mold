#!/bin/bash
source "$(dirname "$0")"/common.inc

# -verbose_deduplicate has ld-prime sum up, when it folds any function,
# the functions folded away and their size, out of every function of
# __TEXT,__text that is in the output (those dead-stripped not counted)
# before folding. Each function here is a 12-byte (arm64) or 16-byte
# (x86-64, padded) atom; f2, f3 and g2 fold into f1 and g1.
cat <<EOF | $CC -o $t/a.o -c -xc - -O2
#define H __attribute__((visibility("hidden"), noinline))
H int f1(int x) { return x * 3 + 1; }
H int f2(int x) { return x * 3 + 1; }
H int f3(int x) { return x * 3 + 1; }
H int g1(int x) { return x * 5 + 7; }
H int g2(int x) { return x * 5 + 7; }
int use(int x) { return f1(x) + f2(x) + f3(x) + g1(x) + g2(x); }
int main() { return use(1); }
EOF

[ $ARCH = arm64 ] && n=12 || n=16
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-O1,-verbose_deduplicate 2> $t/log
total=$(grep '^code deduplicated functions 3 ' $t/log | sed 's/.* out of total \([0-9]*\) (size: \([0-9]*\)).*/\1 \2/')
[ "$(echo $total | cut -d' ' -f1)" = 7 ]
size=$(echo $total | cut -d' ' -f2)
grep -q "^code deduplicated functions 3 (size: $((n * 3))) out of total 7 (size: $size) ([0-9.]*% size reduction)$" $t/log

# use is dead-stripped once main inlines it.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-O1,-verbose_deduplicate,-dead_strip 2> $t/log
grep -q "^code deduplicated functions 3 (size: $((n * 3))) out of total 6 " $t/log

# Nothing folded, nothing said.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-O1,-verbose_deduplicate,-no_deduplicate 2> $t/log
not grep -q 'code deduplicated' $t/log
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-O1,-verbose_deduplicate 2> $t/log
not grep -q 'code deduplicated' $t/log
