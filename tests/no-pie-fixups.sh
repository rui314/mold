#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime gives a non-PIE executable classic dyld info, with lazy
# binding and __mod_init_func, whatever its deployment target; only
# -fixup_chains gets it chains (and __init_offsets). arm64 has no
# non-PIE executables.
[ $ARCH = x86_64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int x = 42;
int *p = &x;
__attribute__((constructor)) static void init(void) { printf("init\n"); }
int main() { printf("%d\n", *p); }
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-no_pie -mmacosx-version-min=14.0 2> /dev/null
otool -l $t/exe1 > $t/lc1
grep -q 'cmd LC_DYLD_INFO_ONLY' $t/lc1
grep -q 'sectname __mod_init_func' $t/lc1
grep -q 'sectname __stub_helper' $t/lc1
$t/exe1 > $t/out1
[ "$(cat $t/out1 | tr '\n' ' ')" = 'init 42 ' ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_pie -Wl,-fixup_chains -mmacosx-version-min=14.0 \
  2> /dev/null
otool -l $t/exe2 > $t/lc2
grep -q 'cmd LC_DYLD_CHAINED_FIXUPS' $t/lc2
grep -q 'sectname __init_offsets' $t/lc2
$t/exe2 > $t/out2
[ "$(cat $t/out2 | tr '\n' ' ')" = 'init 42 ' ]
