#!/bin/bash
source "$(dirname "$0")"/common.inc

# From a deployment target of macOS 15 on, ld-prime folds
# __objc_classrefs into __got: a class reference is an 8-byte slot
# holding the class's address, which is what a GOT entry for the class
# symbol is, so references are redirected to the GOT, identical
# references from different objects share one entry, and the image has
# no __objc_classrefs section (nor the slots' local symbols). Below
# macOS 15 the section stays.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
Class foo_class(void) { return [Foo class]; }
Class array_class(void) { return [NSMutableArray class]; }
EOF2
cat <<EOF2 | $CC -o $t/b.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <stdio.h>
@interface Foo : NSObject
@end
Class foo_class(void);
Class array_class(void);
int main() {
  printf("%s %s %d %d\n", class_getName([Foo class]), class_getName([NSMutableArray class]),
         [Foo class] == foo_class(), [NSMutableArray class] == array_class());
}
EOF2
otool -l $t/a.o | grep -q 'sectname __objc_classrefs'

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=15.0
$t/exe | grep -q '^Foo NSMutableArray 1 1$'
otool -l $t/exe > $t/lc
not grep -q '__objc_classrefs' $t/lc
nm $t/exe > $t/nm
not grep -q 'OBJC_CLASSLIST_REFERENCES' $t/nm
dyld_info -fixups $t/exe > $t/fixups
# One GOT slot per class, bound (NSMutableArray) or rebased (Foo).
[ "$(grep '__got' $t/fixups | grep -c 'OBJC_CLASS_\$_NSMutableArray')" = 1 ]

$CC --ld-path=$mold -o $t/exe14 $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=14.0
$t/exe14 | grep -q '^Foo NSMutableArray 1 1$'
otool -l $t/exe14 | grep -q 'sectname __objc_classrefs'
