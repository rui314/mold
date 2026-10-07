#!/bin/bash
source "$(dirname "$0")"/common.inc

# Merging a category into its class drops it from the category list it
# was in, which keeps its place among the other objects' lists: the
# runtime attaches the categories in list order, so the categories of
# the objects after it still come after its others. Here a.o's list
# loses Foo(F) but keeps NSString(A1) and NSData(D1) ahead of b.o's
# NSArray(R1).
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject @end
@implementation Foo @end
@interface Foo (F) - (int)f; @end
@implementation Foo (F) - (int)f { return 2; } @end
@interface NSString (A1) - (int)a1; @end
@implementation NSString (A1) - (int)a1 { return 1; } @end
@interface NSData (D1) - (int)d1; @end
@implementation NSData (D1) - (int)d1 { return 3; } @end
int foo_f(void) { return [[Foo new] f]; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSArray (R1) - (int)r1; @end
@implementation NSArray (R1) - (int)r1 { return 4; } @end
EOF
cat <<EOF | $CC -o $t/m.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface NSString (A1) - (int)a1; @end
@interface NSData (D1) - (int)d1; @end
@interface NSArray (R1) - (int)r1; @end
int foo_f(void);
int main() {
  printf("%d %d %d %d\n", [@"x" a1], foo_f(), [[NSData data] d1], [@[] r1]);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/m.o -framework Foundation
$RUN $t/exe | grep -q '^1 2 3 4$'
otool -ov $t/exe | sed -n '/__objc_catlist/,/Contents of/p' > $t/log
grep -E '^[0-9a-f]+ ' $t/log | awk '{ print $NF }' | tr '\n' ' ' > $t/order
[ "$(cat $t/order)" = '__OBJC_$_CATEGORY_NSString_$_A1 __OBJC_$_CATEGORY_NSData_$_D1 __OBJC_$_CATEGORY_NSArray_$_R1 ' ]

# Swift labels its category list _objc_categories, which the symbol
# table lists as a local at the list. (ld-prime lists no name of a
# list entry.)
command -v swiftc >/dev/null || exit 0
[ "$ARCH" = "$(uname -m)" ] || exit 0
cat <<EOF2 > $t/ext.swift
import Foundation
extension NSString { @objc public func swA() -> Int { return 1 } }
extension NSData { @objc public func swD() -> Int { return 3 } }
extension NSString { @objc public func swB() -> Int { return 2 } }
EOF2
$SWIFTC -parse-as-library -module-name E -emit-object -o $t/ext.o $t/ext.swift
$CC --ld-path=$mold -shared -o $t/d.dylib $t/ext.o -framework Foundation \
  -L$SDK/usr/lib/swift
nm -m $t/d.dylib > $t/syms
grep -q '(__DATA_CONST,__objc_catlist) non-external _objc_categories$' $t/syms
otool -ov $t/d.dylib | sed -n '/__objc_catlist/,/Contents of/p' > $t/log2
grep -E '^[0-9a-f]+ ' $t/log2 | awk '{ print $NF }' | tr '\n' ' ' > $t/order2
[ "$(cat $t/order2)" = '__CATEGORY_NSString_$_E __CATEGORY_NSData_$_E __CATEGORY_NSString_$_E1 ' ]
