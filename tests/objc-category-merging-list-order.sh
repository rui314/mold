#!/bin/bash
source "$(dirname "$0")"/common.inc

# Categories on a class another image defines merge into the first of
# them. ld-prime drops the merged ones from the category list they were
# in and keeps the list in its place, so the categories of the objects
# after it still come after its others: here NSString(A1), NSData(D1)
# of a.o, then NSArray(R1) of b.o.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSString (A1) - (int)a1; @end
@implementation NSString (A1) - (int)a1 { return 1; } @end
@interface NSData (D1) - (int)d1; @end
@implementation NSData (D1) - (int)d1 { return 3; } @end
@interface NSString (A3) - (int)a3; @end
@implementation NSString (A3) - (int)a3 { return 5; } @end
EOF
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSArray (R1) - (int)r1; @end
@implementation NSArray (R1) - (int)r1 { return 4; } @end
EOF

$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/b.o -framework Foundation
otool -ov $t/c.dylib | sed -n '/__objc_catlist/,/Contents of/p' > $t/log
grep -E '^[0-9a-f]+ ' $t/log | awk '{ print $NF }' | tr '\n' ' ' > $t/order
[ "$(cat $t/order)" = '__OBJC_$_CATEGORY_NSString_$_A1 __OBJC_$_CATEGORY_NSData_$_D1 __OBJC_$_CATEGORY_NSArray_$_R1 ' ]

# Swift labels its category list _objc_categories, which names no
# symbol in the output, the list rebuilt or not.
command -v swiftc >/dev/null || exit 0
[ "$ARCH" = "$(uname -m)" ] || exit 0
cat <<EOF2 > $t/ext.swift
import Foundation
extension NSString { @objc public func swA() -> Int { return 1 } }
extension NSData { @objc public func swD() -> Int { return 3 } }
extension NSString { @objc public func swB() -> Int { return 2 } }
EOF2
swiftc -parse-as-library -module-name E -emit-object -o $t/ext.o $t/ext.swift
$CC --ld-path=$mold -shared -o $t/d.dylib $t/ext.o -framework Foundation \
  -L$(xcrun --show-sdk-path)/usr/lib/swift -Wl,-map,$t/map
nm -m $t/d.dylib > $t/syms
not grep -q _objc_categories $t/syms
not grep -aq _objc_categories $t/map
otool -ov $t/d.dylib | sed -n '/__objc_catlist/,/Contents of/p' > $t/log2
grep -E '^[0-9a-f]+ ' $t/log2 | awk '{ print $NF }' | tr '\n' ' ' > $t/order2
[ "$(cat $t/order2)" = '__CATEGORY_NSString_$_E __CATEGORY_NSData_$_E ' ]
