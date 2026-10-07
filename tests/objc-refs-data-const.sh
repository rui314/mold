#!/bin/bash
source "$(dirname "$0")"/common.inc

# Below macOS 14.4 the Objective-C references stay in __DATA, where the
# runtime may still write them.
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

segments() {
  otool -l $1 | grep -A1 '^  sectname' | awk '/sectname/ {s=$2} /segname/ {print $2 "," s}' |
    grep '__objc_selrefs\|__objc_protorefs\|__objc_classrefs\|__objc_superrefs\|__objc_data\|,__data' |
    sort | tr '\n' ' '
}

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=14.0
$RUN $t/exe | grep -q '^P 7 1$'
[ "$(segments $t/exe)" = '__DATA,__data __DATA,__objc_classrefs __DATA,__objc_data __DATA,__objc_protorefs __DATA,__objc_selrefs __DATA,__objc_superrefs ' ]
$CC --ld-path=$mold -o $t/exe2 $t/a.o -framework Foundation -mmacosx-version-min=14.3
$RUN $t/exe2 | grep -q '^P 7 1$'
[ "$(segments $t/exe2)" = "$(segments $t/exe)" ]

# From macOS 14.4 on they are read-only after dyld's fixups: the
# protocol, class and superclass references move to __DATA_CONST, and
# from macOS 15 on the class references fold into the GOT.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -framework Foundation -mmacosx-version-min=14.4
$RUN $t/exe3 | grep -q '^P 7 1$'
[ "$(segments $t/exe3)" = '__DATA,__data __DATA,__objc_data __DATA,__objc_selrefs __DATA_CONST,__objc_classrefs __DATA_CONST,__objc_protorefs __DATA_CONST,__objc_superrefs ' ]
$CC --ld-path=$mold -o $t/exe4 $t/a.o -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe4 | grep -q '^P 7 1$'
[ "$(segments $t/exe4)" = '__DATA,__data __DATA,__objc_data __DATA,__objc_selrefs __DATA_CONST,__objc_protorefs __DATA_CONST,__objc_superrefs ' ]
