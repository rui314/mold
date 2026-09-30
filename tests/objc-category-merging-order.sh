#!/bin/bash
source "$(dirname "$0")"/common.inc

# Merging a category into its class rewrites the class's ro records
# (class_ro_t, which points at the method, protocol and property
# lists). ld-prime writes the new __OBJC_METACLASS_RO_$_Foo and
# __OBJC_CLASS_RO_$_Foo where the input had them, among the other
# records of __objc_const, and the merged method list sorts by its name
# among the other relative method lists.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c - -mmacosx-version-min=14.0
#import <Foundation/Foundation.h>
#include <stdio.h>
@protocol P - (void)pm; @end
@interface Foo : NSObject <P> { int ivar; } @property int prop; @end
@implementation Foo - (void)pm {} + (void)cm {} @end
@interface Foo (Cat) - (int)catm; @end
@implementation Foo (Cat) - (int)catm { return 42; } @end
int main(void) {
  printf("%d %d\n", [[Foo new] catm], (int)[Foo instancesRespondToSelector:@selector(pm)]);
  return 0;
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=14.0
$t/exe | grep -q '^42 1$'

nm -pm $t/exe | awk '/__objc_const/ { print $NF }' | tr '\n' ' ' > $t/const
grep -q '__OBJC_CLASS_PROTOCOLS_$_Foo __OBJC_METACLASS_RO_$_Foo __OBJC_$_INSTANCE_VARIABLES_Foo __OBJC_$_PROP_LIST_Foo __OBJC_CLASS_RO_$_Foo ' $t/const

if [ $ARCH = arm64 ]; then
  nm -pm $t/exe | awk '/__objc_methlist/ { print $NF }' | tr '\n' ' ' > $t/methlist
  [ "$(cat $t/methlist)" = '__OBJC_$_CLASS_METHODS_Foo __OBJC_$_INSTANCE_METHODS_Foo(Cat) __OBJC_$_PROTOCOL_INSTANCE_METHODS_P ' ]
fi
