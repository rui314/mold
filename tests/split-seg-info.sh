#!/bin/bash
source "$(dirname "$0")"/common.inc

# -add_split_seg_info records where the image refers from one section
# to another (LC_SEGMENT_SPLIT_INFO), so that a dyld shared cache or
# kernel collection builder can slide its segments apart. ld64 and
# ld-prime spell it only so: there is no negative form, and
# -split_seg_info is no option of theirs.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -
#include <stdio.h>
int x = 3;
int *p = &x;
void *ep = &__stderrp;
__attribute__((noinline)) static int helper(int v) { return v + x; }
int f(void) { printf("%d\n", helper(*p)); return x; }
int (*fp)(void) = f;
__attribute__((constructor)) void ctor(void) { puts("ctor"); }
EOF
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.p2align 3
_y: .quad 0
.section __DATA,__const
.p2align 3
_dd: .quad _y - _dd
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o $t/d.o
otool -l $t/b.dylib > $t/lc
not grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc

$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/d.o -Wl,-add_split_seg_info
otool -l $t/c.dylib | awk '$1 == "cmd" { printf "%s ", $2 }' > $t/cmds
grep -q 'LC_SOURCE_VERSION LC_SEGMENT_SPLIT_INFO LC_LOAD_DYLIB' $t/cmds
off() { otool -l $t/c.dylib | grep -A2 "cmd $1\$" | awk '$1 == "dataoff" { print $2 }'; }
[ "$(off LC_SEGMENT_SPLIT_INFO)" -lt "$(off LC_FUNCTION_STARTS)" ]

# Each entry: the referring section and address, the section and
# address referred to, and the kind: a 64-bit pointer (2) or distance
# (4) in data, wherever it points; an adrp (5), the ldr or add under it
# (6) or a branch (7) on arm64, or a 32-bit displacement (3) on x86-64,
# when they cross sections; an image offset (12). Nothing records a
# pointer dyld binds to another image. Xcode 26's dyld_info prints no
# kinds (see split_has).
dyld_info -shared_region $t/c.dylib > $t/split
a() { printf '0x%08x' 0x$(nm $t/c.dylib | awk -v s=$1 '$3 == s { print $1 }'); }
entry() {
  awk -v f=$1 -v fa=$2 -v t=$3 -v ta=$4 -v k=$5 '
    $2 == f && (fa == "-" || $3 == fa) && $5 == t && (ta == "-" || $6 == ta) &&
      (NF == 6 || $7 == k) { n++ }
    END { exit !n }' $t/split
}
if [ $ARCH = arm64 ]; then page=5; pageoff=6; branch=7; else page=3; pageoff=3; branch=3; fi

entry __data $(a _p) __data $(a _x) 2
entry __data $(a _fp) __text $(a _f) 2
entry __const $(a _dd) __data $(a _y) 4
not entry __data $(a _ep) - - 2
entry __text - __data $(a _x) $page
entry __text - __data $(a _x) $pageoff
entry __text - __stubs - $branch
not entry __text - __text - $branch
entry __stubs - __got - $page
entry __stubs - __got - $pageoff
entry __unwind_info - __text - 12
if grep -q 'sectname __init_offsets' $t/lc; then
  entry __init_offsets - __text $(a _ctor) 12
else
  entry __mod_init_func - __text $(a _ctor) 2
fi

for opt in -split_seg_info -no_split_seg_info -no_add_split_seg_info; do
  not $mold -arch $ARCH -dylib -lSystem -syslibroot "$(xcrun --show-sdk-path)" \
    $opt $t/a.o -o $t/d.dylib 2> $t/log
  grep -q "unknown .*option.*$opt" $t/log
done
