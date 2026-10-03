#!/bin/bash
source "$(dirname "$0")"/common.inc

# A class reference is an 8-byte slot in __objc_classrefs holding the
# class's address, which dyld fixes up: identical references from
# different objects coalesce into one slot, as ld64 keeps one class
# reference per class, at any deployment target. (From macOS 15 on,
# ld-prime folds the slots into __got instead.)
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
otool -l $t/a.o | grep 'sectname __objc_classrefs'

for v in 15.0 14.0; do
  $CC --ld-path=$mold -o $t/exe$v $t/a.o $t/b.o -framework Foundation -mmacosx-version-min=$v
  $t/exe$v | grep '^Foo NSMutableArray 1 1$'
  otool -l $t/exe$v | grep -q 'sectname __objc_classrefs'
  dyld_info -fixups $t/exe$v > $t/fixups$v
  [ "$(grep '__objc_classrefs' $t/fixups$v | grep -c 'NSMutableArray')" = 1 ]
done
