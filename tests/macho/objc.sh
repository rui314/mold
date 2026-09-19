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
$t/exe > $t/log
grep greetings $t/log
grep 'hello objc 10' $t/log
