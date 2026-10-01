#!/bin/bash
source "$(dirname "$0")"/common.inc

# -make_mergeable makes a dylib that a later link may merge into its
# image (-merge_*): ld-prime records the dylib's atoms in LC_ATOM_INFO,
# in a format of its own. -add_mergeable_debug_hook gives a debug build
# of such a dylib, which nothing merges, the hook for its classes that
# merged libraries get. Both are for a dylib only, and not together.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 3; }
EOF

for kind in "" -bundle -r; do
  not $mold -arch $ARCH $kind -o $t/out $t/a.o -make_mergeable 2> $t/log1
  grep -q -- '-make_mergeable can only be used when creating a dynamic library' $t/log1
  not $mold -arch $ARCH $kind -o $t/out $t/a.o -add_mergeable_debug_hook 2> $t/log2
  grep -q -- '-add_mergeable_debug_hook can only be used with -dylib' $t/log2
done
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-add_mergeable_debug_hook 2> $t/log3
grep -q -- '-add_mergeable_debug_hook cannot be used with -make_mergeable' $t/log3

# A mergeable dylib's atoms are bound by name in two levels.
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace 2> $t/log4
grep -q -- '-flat_namespace cannot be used with -make_mergeable' $t/log4
$CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace -Wl,-twolevel_namespace 2> $t/log5 || true
not grep -q -- '-flat_namespace cannot' $t/log5

# ld-prime checks the kind of output first, then -r -dead_strip, then
# who gets the debug hook; the conflicts within a dylib after
# -client_name and the shared cache's, before -pagezero_size.
not $mold -arch $ARCH -r -o $t/out $t/a.o -make_mergeable -dead_strip 2> $t/log6
grep -q -- '-make_mergeable can only be used' $t/log6
not $mold -arch $ARCH -r -o $t/out $t/a.o -add_mergeable_debug_hook -dead_strip 2> $t/log7
grep -q -- '-r and -dead_strip cannot be used together' $t/log7
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace -Wl,-add_mergeable_debug_hook -Wl,-pagezero_size,0x1000 2> $t/log8
grep -q -- '-flat_namespace cannot be used with -make_mergeable' $t/log8
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-add_mergeable_debug_hook -Wl,-pagezero_size,0x1000 2> $t/log9
grep -q -- '-add_mergeable_debug_hook cannot be used with -make_mergeable' $t/log9
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace -Wl,-client_name,foo 2> $t/log10
grep -q -- '-client_name can only be used' $t/log10
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace -Wl,-install_name,/usr/lib/liba.dylib 2> $t/log11
grep -q "Shared cache eligible dylibs cannot use '-flat_namespace'" $t/log11

# mold writes no LC_ATOM_INFO, nor the hook, yet.
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
__attribute__((visibility("hidden")))
@interface Hidden : NSObject
@end
@implementation Hidden
@end
EOF
if $mold -v 2>&1 | grep -q mold-macho; then
  not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable 2> $t/log12
  grep -q -- '-make_mergeable is not supported' $t/log12
  not $CC --ld-path=$mold -shared -o $t/b.dylib $t/b.o -framework Foundation \
    -Wl,-add_mergeable_debug_hook 2> $t/log13
  grep -q -- '-add_mergeable_debug_hook is not supported' $t/log13
else
  $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable
  otool -l $t/a.dylib | grep -q LC_ATOM_INFO
  $CC --ld-path=$mold -shared -o $t/b.dylib $t/b.o -framework Foundation \
    -Wl,-add_mergeable_debug_hook
  nm $t/b.dylib | grep -q imageNameHook
fi
