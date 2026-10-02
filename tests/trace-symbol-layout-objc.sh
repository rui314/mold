#!/bin/bash
source "$(dirname "$0")"/common.inc

# -trace_symbol_layout reports where the Objective-C records the
# linker rewrites go: a method list rewritten in the relative form goes
# to __TEXT,__objc_methlist (an x86-64 executable keeps its lists
# absolute), else it stays in __DATA,__objc_const; a class merged with
# its category keeps its records there too. (ld-prime also reports the
# lists category merging makes, in words of its own.)
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
if [ $ARCH = arm64 ]; then
  grep -Fx "symbol '__OBJC_\$_CLASS_METHODS_B', mapped to __TEXT/__objc_methlist" $t/log
else
  grep -Fx "symbol '__OBJC_\$_CLASS_METHODS_B', mapped to __DATA/__objc_const" $t/log
fi
grep -Fx "symbol '__OBJC_CLASS_RO_\$_A', mapped to __DATA/__objc_const" $t/log
grep -Fx "symbol '-[A(Cat) baz]', mapped to __TEXT/__text" $t/log

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-trace_symbol_layout \
  -Wl,-no_objc_relative_method_lists > $t/log2
grep -Fx "symbol '__OBJC_\$_CLASS_METHODS_B', mapped to __DATA/__objc_const" $t/log2
