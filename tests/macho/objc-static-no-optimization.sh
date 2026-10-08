#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime optimizes the Objective-C of no image dyld doesn't load (a
# -static or -preload one, a kext): it converts no method list to the
# relative form (whatever -objc_relative_method_lists says), merges no
# category into its class and folds no class reference into the GOT.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c - -mmacos-version-min=15.0 -Wno-objc-root-class
@interface Foo { void *isa; }
- (int)bar;
+ (int)cbar;
@end
@implementation Foo
- (int)bar { return 1; }
+ (int)cbar { return 2; }
@end
@interface Foo (Cat)
- (int)baz;
@end
@implementation Foo (Cat)
- (int)baz { return 3; }
@end
int f(id x) { return [x bar] + [x baz] + [Foo cbar]; }
void start(void) {}
EOF
cat <<EOF | $CC -o $t/rt.o -c -xassembler -
.globl __objc_empty_cache, _objc_msgSend
.data
__objc_empty_cache: .quad 0
.text
_objc_msgSend: ret
EOF

for opt in -static -preload; do
  $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 15.0 15.0} $opt -e _start \
    -objc_relative_method_lists -o $t/exe $t/a.o $t/rt.o
  otool -l $t/exe > $t/load
  not grep -F __objc_methlist $t/load
  grep -F __objc_catlist $t/load
  grep -F __objc_classrefs $t/load
  nm $t/exe | grep -F '__OBJC_$_CATEGORY_Foo_$_Cat'
done
