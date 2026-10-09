#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# ld-prime's defaults for the fixup format (measured): chained fixups
# from macOS 12, except for an x86_64 executable, from macOS 13;
# classic dyld info below that; -undefined dynamic_lookup (and
# suppress) turn the default back to classic dyld info, -undefined
# warning (a deprecated no-op) does not; an explicit -fixup_chains
# always wins. Hammerspoon (deployment target
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
$RUN $t/hi | grep hi

$CC --ld-path=$mold -o $t/dl $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,dynamic_lookup
[ "$(fmt $t/dl)" = LC_DYLD_INFO_ONLY ]
# ...but the initializer layout still follows the deployment target:
# __init_offsets, as ld-prime lays it out. Only an explicit
# -no_fixup_chains keeps __mod_init_func, and an explicit -fixup_chains
# brings __init_offsets below that deployment target too.
[ "$(init $t/dl)" = __init_offsets ]
$CC --ld-path=$mold -o $t/nfc $t/a.o -mmacosx-version-min=$hi -Wl,-no_fixup_chains
[ "$(init $t/nfc)" = __mod_init_func ]
$CC --ld-path=$mold -o $t/fc $t/a.o -mmacosx-version-min=$lo -Wl,-fixup_chains
[ "$(init $t/fc)" = __init_offsets ]
$CC --ld-path=$mold -o $t/dl2 $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,dynamic_lookup -Wl,-fixup_chains
[ "$(fmt $t/dl2)" = LC_DYLD_CHAINED_FIXUPS ]
$CC --ld-path=$mold -o $t/wn $t/a.o -mmacosx-version-min=$hi -Wl,-undefined,warning 2> /dev/null
[ "$(fmt $t/wn)" = LC_DYLD_CHAINED_FIXUPS ]

# An x86_64 dylib or bundle goes chained from macOS 12 as on arm64;
# only an x86_64 executable waits for 13.
$CC --ld-path=$mold -o $t/lo.dylib -shared $t/a.o -mmacosx-version-min=11.0
[ "$(fmt $t/lo.dylib)" = LC_DYLD_INFO_ONLY ]
$CC --ld-path=$mold -o $t/hi.dylib -shared $t/a.o -mmacosx-version-min=12.0
[ "$(fmt $t/hi.dylib)" = LC_DYLD_CHAINED_FIXUPS ]
[ "$(init $t/hi.dylib)" = __init_offsets ]
$CC --ld-path=$mold -o $t/hi.bundle -bundle $t/a.o -mmacosx-version-min=12.0
[ "$(fmt $t/hi.bundle)" = LC_DYLD_CHAINED_FIXUPS ]
