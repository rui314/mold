#!/bin/bash
source "$(dirname "$0")"/common.inc

# -make_mergeable makes a dylib that a later link may merge into its
# image (-merge_*): ld-prime records the dylib's subsections in a
# mergeable record of its own format, which LC_ATOM_INFO points at.
# -add_mergeable_debug_hook gives a debug build of such a dylib, which
# nothing merges, the hook for its classes that merged libraries get.
# Both are for a dylib only, and not together.
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

# A mergeable dylib's imports are bound by name in two levels.
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace 2> $t/log4
grep -q -- '-flat_namespace cannot be used with -make_mergeable' $t/log4
$CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace -Wl,-twolevel_namespace 2> $t/log5 || true
not grep -q -- '-flat_namespace cannot' $t/log5

# Only a dylib is mergeable, or gets the debug hook, which a mergeable
# dylib can't have; nor can it bind flat.
not $mold -arch $ARCH -r -o $t/out $t/a.o -make_mergeable 2> $t/log6
grep -q -- '-make_mergeable can only be used' $t/log6
not $mold -arch $ARCH -r -o $t/out $t/a.o -add_mergeable_debug_hook 2> $t/log7
grep -q -- '-add_mergeable_debug_hook can only be used with -dylib' $t/log7
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-flat_namespace 2> $t/log8
grep -q -- '-flat_namespace cannot be used with -make_mergeable' $t/log8
not $CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-add_mergeable_debug_hook 2> $t/log9
grep -q -- '-add_mergeable_debug_hook cannot be used with -make_mergeable' $t/log9

# The mergeable record is at the start of __LINKEDIT after the
# function starts and data in code, and its load command follows
# theirs.
$CC --ld-path=$mold -shared -o $t/a.dylib $t/a.o -Wl,-make_mergeable
otool -l $t/a.dylib > $t/cmds
grep -A3 LC_ATOM_INFO $t/cmds > $t/record-cmd
grep -B5 LC_ATOM_INFO $t/cmds | grep -q LC_DATA_IN_CODE
dataoff=$(grep dataoff $t/record-cmd | awk '{print $2}')
dice=$(grep -A3 LC_DATA_IN_CODE $t/cmds | grep dataoff | awk '{print $2}')
dicesize=$(grep -A3 LC_DATA_IN_CODE $t/cmds | grep datasize | awk '{print $2}')
[ $dataoff = $((dice + dicesize)) ]
dd if=$t/a.dylib bs=1 skip=$dataoff count=8 2> /dev/null | grep -q nldprecr

# Its code is linked again where it is merged, by the fixups of what
# the objects had: an applied optimization hint would have changed it.
cat <<EOF | $CC -o $t/c.o -c -O2 -xc -
int counter;
int get(void) { return counter; }
EOF
$CC --ld-path=$mold -shared -o $t/c.dylib $t/c.o -Wl,-make_mergeable
otool -tv $t/c.dylib > $t/disasm
not grep -q 'nop' $t/disasm

# The debug build gets the hook for the classes it doesn't export (see
# merged-libraries-hook.sh), after the export options; none if there
# are none. The hook's table of classes is mold's ___mold_bundle_hook_table
# or ld-prime's _relinkableLibraryClasses.
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
__attribute__((visibility("hidden")))
@interface Hidden : NSObject
@end
@implementation Hidden
@end
@interface Shown : NSObject
@end
@implementation Shown
@end
EOF
cat <<EOF | $CC -o $t/d.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Shown : NSObject
@end
@implementation Shown
@end
EOF
$CC --ld-path=$mold -shared -o $t/b.dylib $t/b.o -framework Foundation \
  -Wl,-add_mergeable_debug_hook
nm $t/b.dylib > $t/syms
grep -Eq '___mold_bundle_hook_table|_relinkableLibraryClasses' $t/syms
for objs in $t/d.o $t/a.o; do
  $CC --ld-path=$mold -shared -o $t/d.dylib $objs -framework Foundation \
    -Wl,-add_mergeable_debug_hook
  nm $t/d.dylib > $t/syms
  not grep -Eq '___mold_bundle_hook_table|_relinkableLibraryClasses' $t/syms
done
$CC --ld-path=$mold -shared -o $t/d.dylib $t/d.o -framework Foundation \
  -Wl,-add_mergeable_debug_hook -Wl,-unexported_symbol,'_OBJC_CLASS_$_Shown'
nm $t/d.dylib > $t/syms
grep -Eq '___mold_bundle_hook_table|_relinkableLibraryClasses' $t/syms
$CC --ld-path=$mold -shared -o $t/d.dylib $t/b.o -framework Foundation \
  -Wl,-add_mergeable_debug_hook -Wl,-no_merged_libraries_hook
nm $t/d.dylib > $t/syms
not grep -Eq '___mold_bundle_hook_table|_relinkableLibraryClasses' $t/syms
