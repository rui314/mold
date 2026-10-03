#!/bin/bash
source "$(dirname "$0")"/common.inc

# The same inputs always produce the same bytes: the class references
# and whatever the link makes of them must not follow the order of a
# hash map, which hashbrown reseeds every process.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
id f(void) {
  return @[ [NSString class], [NSArray class], [NSDictionary class],
            [NSNumber class], [NSData class], [NSDate class],
            [NSSet class], [NSError class], [NSValue class],
            [NSMutableArray class], [NSMutableDictionary class] ];
}
EOF2

# Link the same inputs to the same path twice (so the code signature
# identifier and the content-derived UUID are held fixed) and compare.
$CC -mmacosx-version-min=15.0 --ld-path=$mold -dynamiclib -o $t/out.dylib $t/a.o -framework Foundation
cp $t/out.dylib $t/first.dylib
$CC -mmacosx-version-min=15.0 --ld-path=$mold -dynamiclib -o $t/out.dylib $t/a.o -framework Foundation
cmp $t/first.dylib $t/out.dylib
