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

# From macOS 14.4 on they are read-only after dyld's fixups: the
# protocol, class and superclass references move to __DATA_CONST in
# that order, and from macOS 15 on the class references fold into the
# GOT.
segment_order() {
  otool -l $1 | grep -A1 '^  sectname' | awk '/sectname/ {s=$2} /segname/ {print $2 "," s}' |
    grep '__objc_protorefs\|__objc_classrefs\|__objc_superrefs' | tr '\n' ' '
}
$CC --ld-path=$mold -o $t/exe2 $t/a.o -framework Foundation -mmacosx-version-min=14.3
$t/exe2 | grep -q '^P 7 1$'
[ "$(segment_order $t/exe2)" = \
  '__DATA,__objc_protorefs __DATA,__objc_classrefs __DATA,__objc_superrefs ' ]
$CC --ld-path=$mold -o $t/exe3 $t/a.o -framework Foundation -mmacosx-version-min=14.4
$t/exe3 | grep -q '^P 7 1$'
[ "$(segment_order $t/exe3)" = \
  '__DATA_CONST,__objc_protorefs __DATA_CONST,__objc_classrefs __DATA_CONST,__objc_superrefs ' ]
$CC --ld-path=$mold -o $t/exe4 $t/a.o -framework Foundation -mmacosx-version-min=15.0
$t/exe4 | grep -q '^P 7 1$'
[ "$(segment_order $t/exe4)" = '__DATA_CONST,__objc_protorefs __DATA_CONST,__objc_superrefs ' ]
