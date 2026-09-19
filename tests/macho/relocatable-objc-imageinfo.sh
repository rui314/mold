#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output must carry the merged __objc_imageinfo record. dyld only
# hands an image to the Objective-C runtime if it has one, so a
# prelinked object without it makes the final image's classes and
# categories invisible: [Foo class] on a class defined in such an
# image aborts with "Attempt to use unknown class", and categories on
# framework classes never attach (NetNewsWire's SPM packages, prelinked
# with -r, crashed both ways).
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
EOF
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSObject (Extra)
- (int)extraMethod;
@end
@implementation NSObject (Extra)
- (int)extraMethod { return 42; }
@end
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
otool -l $t/r.o > $t/otool
grep -A2 'sectname __objc_imageinfo' $t/otool | grep -q 'segname __DATA'
grep -A3 'sectname __objc_imageinfo' $t/otool | grep -q 'size 0x0000000000000008'
# Exactly one merged record.
[ "$(grep -c 'sectname __objc_imageinfo' $t/otool)" = 1 ]

cat <<EOF | $CC -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@interface Foo : NSObject
@end
@interface NSObject (Extra)
- (int)extraMethod;
@end
int main() {
  printf("%d %d %d\n", objc_getClass("Foo") != NULL, [Foo class] != nil,
         [[Foo alloc] extraMethod]);
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -framework Foundation
$t/exe | grep -q '^1 1 42$'

# The prelinked object goes through a dylib too.
$CC --ld-path=$mold -dynamiclib -o $t/libfoo.dylib $t/r.o -framework Foundation \
  -install_name @rpath/libfoo.dylib
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/libfoo.dylib -framework Foundation \
  -Wl,-rpath,$t
$t/exe2 | grep -q '^1 1 42$'
