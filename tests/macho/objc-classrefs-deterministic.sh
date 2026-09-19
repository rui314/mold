#!/bin/bash
source "$(dirname "$0")"/common.inc

# Folding __objc_classrefs into __got must assign the GOT slots in a
# fixed order, so the same inputs always produce the same bytes. The
# slots used to be walked in the order of a HashMap that hashbrown
# reseeds every process, so two links of one object placed the class
# GOT entries differently - every load that encodes a slot address,
# and the __got contents, then varied run to run. Slots are now
# assigned in the object's class-reference order.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id f(void) {
  return @[ [NSString class], [NSArray class], [NSDictionary class],
            [NSNumber class], [NSData class], [NSDate class],
            [NSSet class], [NSError class], [NSValue class],
            [NSMutableArray class], [NSMutableDictionary class] ];
}
EOF2

# A macOS 15 deployment target turns class references into GOT loads.
# Link the same inputs to the same path twice (so the code signature
# identifier and the content-derived UUID are held fixed) and compare.
$CC -mmacosx-version-min=15.0 --ld-path=$mold -dynamiclib -o $t/out.dylib $t/a.o -framework Foundation
cp $t/out.dylib $t/first.dylib
$CC -mmacosx-version-min=15.0 --ld-path=$mold -dynamiclib -o $t/out.dylib $t/a.o -framework Foundation
cmp $t/first.dylib $t/out.dylib
