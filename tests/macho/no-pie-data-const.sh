#!/bin/bash
source "$(dirname "$0")"/common.inc

# dyld makes __DATA_CONST read-only once it has applied an image's
# fixups. ld-prime gives a non-PIE executable no such segment unless
# -data_const asks for it, chained fixups or not: its GOT and
# initializer pointers stay in __DATA. arm64 has no non-PIE
# executables.
[ $ARCH = x86_64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int x = 42;
int *const p = &x;
__attribute__((constructor)) static void init(void) { printf("init\n"); }
int main() { printf("%d\n", *p); }
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-no_pie -mmacosx-version-min=11.0
otool -l $t/exe1 > $t/lc1
not grep -q 'segname __DATA_CONST' $t/lc1
$RUN $t/exe1 > $t/out1
[ "$(cat $t/out1 | tr '\n' ' ')" = 'init 42 ' ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_pie -Wl,-fixup_chains -mmacosx-version-min=14.0 \
  2> /dev/null
otool -l $t/exe2 > $t/lc2
not grep -q 'segname __DATA_CONST' $t/lc2
$RUN $t/exe2 > $t/out2
[ "$(cat $t/out2 | tr '\n' ' ')" = 'init 42 ' ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-no_pie -Wl,-data_const -mmacosx-version-min=11.0
otool -l $t/exe3 > $t/lc3
grep -q 'segname __DATA_CONST' $t/lc3

# Not even when bound for the shared region.
$CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-no_pie -Wl,-add_split_seg_info \
  -mmacosx-version-min=11.0
otool -l $t/exe5 > $t/lc5
not grep -q 'segname __DATA_CONST' $t/lc5

$CC --ld-path=$mold -o $t/exe4 $t/a.o -mmacosx-version-min=11.0
otool -l $t/exe4 > $t/lc4
grep -q 'segname __DATA_CONST' $t/lc4
