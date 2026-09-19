#!/bin/bash
source "$(dirname "$0")"/common.inc

# A dylib every reference to which is a weak import is loaded with
# LC_LOAD_WEAK_DYLIB and its binds carry the weak-import flag; one
# strong reference to a symbol makes the symbol - and the dylib's load
# - strong, whatever other objects say (ld64's default,
# -weak_reference_mismatches non-weak). Binding to a dylib's weak
# definition sets MH_BINDS_TO_WEAK (0x10000) on the image. All as
# ld-prime does.
cat <<EOF2 | $CC -o $t/lib.o -c -xc -
int wf(void) { return 1; }
int sf(void) { return 2; }
__attribute__((weak)) int wk(void) { return 3; }
EOF2
$CC --ld-path=$mold -dynamiclib -o $t/libwl.dylib $t/lib.o -install_name @rpath/libwl.dylib
cat <<EOF2 | $CC -o $t/w1.o -c -xc -
#include <stdio.h>
extern int wf(void) __attribute__((weak_import));
int main() { printf("%d\n", wf ? wf() : 0); return 0; }
EOF2
cat <<EOF2 | $CC -o $t/w2.o -c -xc -
int wf(void);
int g(void) { return wf(); }
EOF2
cat <<EOF2 | $CC -o $t/w3.o -c -xc -
#include <stdio.h>
int wk(void);
int main() { printf("%d\n", wk()); return 0; }
EOF2
load() { otool -L $1 | grep libwl | awk '{print ($NF=="weak)") ? "weak" : "strong"}'; }
flags() { otool -h $1 | tail -1 | awk '{print $NF}'; }

$CC --ld-path=$mold -o $t/e1 $t/w1.o $t/libwl.dylib -Wl,-rpath,$t
[ "$(load $t/e1)" = weak ]
dyld_info -fixups $t/e1 | grep 'libwl/_wf' | grep -q 'weak-import'
[ "$(flags $t/e1)" = 0x00200085 ]
$t/e1 | grep -q '^1$'

$CC --ld-path=$mold -o $t/e2 $t/w1.o $t/w2.o $t/libwl.dylib -Wl,-rpath,$t
[ "$(load $t/e2)" = strong ]
dyld_info -fixups $t/e2 > $t/fixups2
grep -q 'libwl/_wf' $t/fixups2
not grep -q 'weak-import' $t/fixups2
$t/e2 | grep -q '^1$'

$CC --ld-path=$mold -o $t/e3 $t/w3.o $t/libwl.dylib -Wl,-rpath,$t
[ "$(load $t/e3)" = strong ]
[ "$(flags $t/e3)" = 0x00210085 ]
$t/e3 | grep -q '^3$'
