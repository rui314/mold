#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Greeter : NSObject
- (void)greet;
@end
@implementation Greeter
- (void)greet { printf("greetings\n"); }
@end
int main() {
  @autoreleasepool {
    Greeter *g = [[Greeter alloc] init];
    [g greet];
    NSString *s = @"hello objc";
    printf("%s %lu\n", s.UTF8String, (unsigned long)s.length);
  }
}
EOF2

$CC --ld-path=$mold -framework Foundation -o $t/exe $t/a.o
# ld-prime converts method lists to relative form in every arm64
# image, and on x86-64 in dylibs and bundles only: an x86-64
# executable keeps the compiler's absolute lists in __objc_const.
otool -l $t/exe > $t/sections
if [ $ARCH = arm64 ]; then
  grep -q 'sectname __objc_methlist' $t/sections
else
  not grep -q 'sectname __objc_methlist' $t/sections
fi
$CC --ld-path=$mold -framework Foundation -dynamiclib -o $t/libgreet.dylib $t/a.o
otool -l $t/libgreet.dylib | grep 'sectname __objc_methlist'
$t/exe > $t/log
grep greetings $t/log
grep 'hello objc 10' $t/log
