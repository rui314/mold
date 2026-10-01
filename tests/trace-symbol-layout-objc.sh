#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_symbol_layout reports the lists category merging makes, named
# after the class and its categories, after the Objective-C stubs and
# before the method lists ld-prime rewrites in the relative form: where
# relative method lists go, in __objc_methlist; else in __objc_data.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c - -mmacos-version-min=13.0
#import <Foundation/Foundation.h>
@interface B : NSObject
+ (int)cqux;
@end
@implementation B
+ (int)cqux { return 2; }
@end
@interface A : NSObject
- (int)bar;
@end
@implementation A
- (int)bar { return 1; }
@end
@interface A (Cat)
- (int)baz;
+ (int)cbaz;
@end
@implementation A (Cat)
- (int)baz { return 3; }
+ (int)cbaz { return 4; }
@end
int main() { return [[A new] baz]; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-trace_symbol_layout > $t/log
grep -F "symbol '__OBJC_\$_INSTANCE_METHODS_A(Cat)', use default mapping to " $t/log
grep -F "symbol '__OBJC_\$_CLASS_METHODS_A(Cat)', use default mapping to " $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-trace_symbol_layout \
  -Wl,-no_objc_relative_method_lists > $t/log2
grep -Fx "symbol '__OBJC_\$_INSTANCE_METHODS_A(Cat)', use default mapping to __DATA/__objc_data" $t/log2
grep -Fx "symbol '__OBJC_\$_CLASS_METHODS_A(Cat)', use default mapping to __DATA/__objc_data" $t/log2

[ $ARCH = arm64 ] || exit 0
sed 's/, use.*//' $t/log | tail -4 | tr '\n' ' ' > $t/order
grep -F "symbol '_objc_msgSend\$baz' symbol '__OBJC_\$_INSTANCE_METHODS_A(Cat)' symbol '__OBJC_\$_CLASS_METHODS_A(Cat)' symbol '__OBJC_\$_CLASS_METHODS_B' " $t/order
