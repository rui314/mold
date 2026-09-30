#!/bin/bash
source "$(dirname "$0")"/common.inc

# Below macOS 15 the Objective-C references stay in __DATA, in
# ld-prime's order: __objc_selrefs, __objc_protorefs, __objc_classrefs,
# __objc_superrefs, then __objc_data before the program's __data.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -fno-objc-msgsend-selector-stubs -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
@protocol P
- (int)pm;
@end
@interface Foo : NSObject <P>
@end
@implementation Foo
- (id)init { return [super init]; }
- (int)pm { return 7; }
@end
int data = 1;
int main() {
  printf("%s %d %d\n", protocol_getName(@protocol(P)), [[Foo new] pm], data);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=14.0
$t/exe | grep -q '^P 7 1$'
otool -l $t/exe | grep -A1 '^  sectname' | awk '/sectname/ {s=$2} /segname __DATA$/ {print s}' \
  > $t/order
[ "$(grep '__objc_selrefs\|__objc_protorefs\|__objc_classrefs\|__objc_superrefs\|__objc_data\|__data' \
  $t/order | tr '\n' ' ')" = \
  '__objc_selrefs __objc_protorefs __objc_classrefs __objc_superrefs __objc_data __data ' ]
