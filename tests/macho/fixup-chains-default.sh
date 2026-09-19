#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's defaults for the fixup format (measured): chained fixups
# from macOS 12 on arm64 and from macOS 13 on x86_64, classic dyld
# info below that; -undefined dynamic_lookup (and suppress) turn the
# default back to classic dyld info, -undefined warning does not; an
# explicit -fixup_chains always wins. Hammerspoon (deployment target
# 13, -undefined dynamic_lookup) came out chained from us and classic
# from ld-prime.
cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((constructor)) static void init(void) {}
int main() { printf("hi\n"); return 0; }
EOF2

fmt() { otool -l $1 | grep -E 'LC_DYLD_INFO|LC_DYLD_CHAINED' | awk '{print $2}'; }
init() { otool -l $1 | grep -E 'sectname (__init_offsets|__mod_init_func)' | awk '{print $2}'; }

if [ $ARCH = arm64 ]; then lo=11.0; hi=12.0; else lo=12.0; hi=13.0; fi
$CC --ld-path=$mold -o $t/lo $t/a.o -mmacosx-version-min=$lo
[ "$(fmt $t/lo)" = LC_DYLD_INFO_ONLY ]
[ "$(init $t/lo)" = __mod_init_func ]
$CC --ld-path=$mold -o $t/hi $t/a.o -mmacosx-version-min=$hi
[ "$(fmt $t/hi)" = LC_DYLD_CHAINED_FIXUPS ]
[ "$(init $t/hi)" = __init_offsets ]
$t/hi | grep -q hi

$CC --ld-path=$mold -o $t/dl $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,dynamic_lookup
[ "$(fmt $t/dl)" = LC_DYLD_INFO_ONLY ]
$CC --ld-path=$mold -o $t/dl2 $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,dynamic_lookup -Wl,-fixup_chains
[ "$(fmt $t/dl2)" = LC_DYLD_CHAINED_FIXUPS ]
$CC --ld-path=$mold -o $t/wn $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,warning 2> /dev/null
[ "$(fmt $t/wn)" = LC_DYLD_CHAINED_FIXUPS ]
